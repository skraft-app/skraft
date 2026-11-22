//! Whole scenarios driven like the app does: pixels in, operations out.

use glam::{DVec2, DVec3};
use skraft_model::{Boolean, Document, EntityPath, Id, Operation, Primitive};

use crate::draw::ThemeColor;
use crate::*;

/// An editor looking straight down (parallel), framing ±1 m.
fn top_editor(doc: Document) -> Editor {
    let mut e = Editor::new(doc, Box::new(skraft_kernel::Inline::default()));
    e.set_viewport_size(DVec2::new(1000.0, 1000.0));
    e.set_parallel(true);
    e.camera.set_standard_view(
        StandardView::Top,
        glam::DMat3::IDENTITY,
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
        return part.machinings().last().unwrap().id;
    }
    doc.assembly(doc.context_of(e.context()).unwrap()).unwrap().occurrences().last().unwrap().id
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
fn typed_values_show_in_the_bubbles() {
    let mut e = top_editor(Document::new());
    e.set_tool(ToolKind::Box);
    click(&mut e, DVec3::ZERO);
    hover(&mut e, DVec3::new(300.0, 200.0, 0.0));
    // The text typed, then the result after `|`.
    let written = |l: &PreviewLabel| l.result.as_ref().map_or(l.text.clone(), |r| format!("{}|= {r}", l.text));
    let shown = |e: &Editor, typed: &str| e.labels(typed).iter().map(|l| (written(l), l.active)).collect::<Vec<_>>();
    let s = |t: &str, active: bool| (t.to_owned(), active);
    assert_eq!(shown(&e, ""), [s("300", false), s("200", false)]);
    // The red side first; `;` moves on to the green one.
    assert_eq!(shown(&e, "600"), [s("600", true), s("200", false)]);
    assert_eq!(shown(&e, "600;"), [s("600", false), s("", true)]);
    assert_eq!(shown(&e, "600;@/2"), [s("600", false), s("@/2|= 100", true)]);
    // The height typed with the base has no bubble yet: the box only.
    assert_eq!(shown(&e, "600;400;19"), [s("600", false), s("400", false)]);
    // For the offset option (`<length>x`): the bubbles keep the cursor's measures.
    assert_eq!(shown(&e, "5x"), [s("300", false), s("200", false)]);

    // Not moved yet, the move's length shows only while typed.
    let mut e = top_editor(Document::new());
    draw_box(&mut e, DVec3::ZERO, DVec3::new(300.0, 200.0, 0.0), "100");
    e.set_tool(ToolKind::Select);
    click(&mut e, DVec3::new(150.0, 100.0, 100.0));
    e.set_tool(ToolKind::Move);
    click(&mut e, DVec3::new(300.0, 200.0, 100.0));
    assert!(shown(&e, "").is_empty());
    assert_eq!(shown(&e, "50"), [s("50", true)]);
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
    // Moved, let go (docs/tools/selection.md): picked again, then the start.
    click(&mut e, DVec3::new(10.0, 20.0, 100.0));
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

#[test]
fn shape_gauge_shows_offset_and_fillet() {
    let mut e = top_editor(Document::new());
    // Before the first click, the gauge at the cursor starts on it; an offset out grows it past
    // the cursor, one in keeps it inside (the outline drawn dashed on the cursor); a fillet
    // rounds it (askew segments). Box, then prism and cylinder.
    let at = DVec3::new(100.0, 100.0, 0.0);
    let lines = |e: &mut Editor| {
        hover(e, at);
        e.preview().drawing.lines
    };
    let past = |l: &[crate::draw::Line]| l.iter().any(|l| l.a.x.min(l.b.x) < at.x - 1e-6);
    let askew = |l: &[crate::draw::Line]| l.iter().any(|l| (l.a.x - l.b.x).abs() > 1e-6 && (l.a.y - l.b.y).abs() > 1e-6);
    e.set_tool(ToolKind::Box);
    let plain = lines(&mut e);
    assert!(!past(&plain) && !askew(&plain));
    e.input(ToolInput::Vcb("5x".into()));
    assert!(past(&lines(&mut e)));
    e.input(ToolInput::Vcb("-5x".into()));
    let inside = lines(&mut e);
    assert!(!past(&inside) && inside.len() > plain.len());
    e.input(ToolInput::Vcb("0x".into()));
    e.input(ToolInput::Vcb("5r".into()));
    assert!(askew(&lines(&mut e)));

    e.set_tool(ToolKind::Prism);
    assert!(!past(&lines(&mut e)));
    e.input(ToolInput::Vcb("5x".into()));
    assert!(past(&lines(&mut e)));

    // The offset is shared (as OCL's): the prism's back to none, then the cylinder's in.
    e.input(ToolInput::Vcb("0x".into()));
    e.set_tool(ToolKind::Cylinder);
    let plain = lines(&mut e);
    e.input(ToolInput::Vcb("-5x".into()));
    assert!(lines(&mut e).len() > plain.len());
}

#[test]
fn shape_offset_moves_the_outline() {
    let mut e = top_editor(Document::new());
    // `5x` at the first step sets the box's offset (saved, shared): the base drawn from
    // (0,0) to (300,200) is made 5 larger on every side; the height is untouched.
    e.set_tool(ToolKind::Box);
    assert!(e.takes_typing());
    e.input(ToolInput::Vcb("5x".into()));
    let offset = |e: &Editor| e.tool_options().into_iter().find(|o| o.key == "offset").map(|o| o.value);
    assert_eq!(offset(&e), Some(OptionValue::Length(5.0)));
    let (lo, hi) = box_bounds(&mut e, DVec3::ZERO, DVec3::new(300.0, 200.0, 0.0), "150");
    assert!(close(lo, DVec3::new(-5.0, -5.0, 0.0)) && close(hi, DVec3::new(305.0, 205.0, 150.0)), "{lo} {hi}");

    // Typed in the bar, `@` the current value; at the height step `-@x` shrinks the base
    // drawn the other way (corners clicked right to left).
    e.type_tool_length("offset", "@*2");
    assert_eq!(offset(&e), Some(OptionValue::Length(10.0)));
    click(&mut e, DVec3::new(600.0, 200.0, 0.0));
    click(&mut e, DVec3::new(400.0, 0.0, 0.0));
    e.input(ToolInput::Vcb("-@x".into()));
    e.input(ToolInput::Vcb("50".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(close(lo, DVec3::new(410.0, 10.0, 0.0)) && close(hi, DVec3::new(590.0, 190.0, 50.0)), "{lo} {hi}");

    // An offset leaving nothing says so and makes nothing; a bad one says why.
    e.input(ToolInput::Vcb("-100x".into()));
    let before = drawn(&e);
    click(&mut e, DVec3::new(-500.0, -500.0, 0.0));
    e.input(ToolInput::Vcb("100;100;10".into()));
    assert_eq!(drawn(&e), before);
    assert_eq!(e.status(), "Le décalage ne laisse rien de la forme");
    e.input(ToolInput::Vcb("2°x".into()));
    assert!(e.status().starts_with("Décalage — « 2° »"), "{}", e.status());
    assert_eq!(offset(&e), Some(OptionValue::Length(-100.0)));

    // The cylinder shares it (as OCL's, docs/tools/draw-options.md); its radius grows by it, in Diameter
    // mode too.
    e.set_tool(ToolKind::Cylinder);
    assert_eq!(offset(&e), Some(OptionValue::Length(-100.0)));
    e.input(ToolInput::Vcb("3x".into()));
    click(&mut e, DVec3::new(-400.0, 600.0, 0.0));
    e.input(ToolInput::Vcb("50;10".into()));
    let (lo, _) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!((lo.x + 347.0).abs() < 1e-3, "radius 53: {lo}");
    e.input(ToolInput::Key { key: Key::Tab, mods: Modifiers::default() });
    click(&mut e, DVec3::new(-200.0, 600.0, 0.0));
    e.input(ToolInput::Vcb("50;10".into()));
    let (lo, _) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!((lo.x + 172.0).abs() < 1e-3, "diameter 50, radius 28: {lo}");

    // Move keeps `5x` for its copies.
    e.set_tool(ToolKind::Move);
    assert!(e.tool_options().iter().all(|o| o.key != "offset"));
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
    use skraft_model::GuideShape;
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
    use skraft_model::GuideShape;
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
    use skraft_model::GuideShape;
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
    use skraft_model::GuideShape;
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
    let at = |p: DVec3| e.preview().drawing.lines.iter().any(|l| (l.a.truncate() - p.truncate()).length() < 1e-3);
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
        assert!(!e.preview().drawing.lines.is_empty());
        e.leave();
        assert!(e.preview().drawing.lines.is_empty());
        // Back in the view: shown again.
        e.input(ToolInput::Hover { px: px(&e, DVec3::new(100.0, 100.0, 0.0)) });
        assert!(!e.preview().drawing.lines.is_empty());
    }
}

#[test]
fn the_height_takes_the_last_one_the_next_face_or_both_sides() {
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    draw_box(&mut e, DVec3::ZERO, DVec3::new(300.0, 200.0, 0.0), "150");
    // A block floating above the middle of `a`'s top.
    let block = Primitive::Box { origin: DVec3::new(120.0, 70.0, 350.0), size: DVec3::new(60.0, 60.0, 50.0) };
    e.apply(Operation::AddPrimitive { context: root, primitive: block, placement: glam::DAffine3::IDENTITY, material: None }).unwrap();

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
    e.apply(Operation::AddPrimitive { context: root, primitive: disc, placement: glam::DAffine3::IDENTITY, material: None }).unwrap();
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
    use skraft_i18n::t;
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
    // Moved, the selection is let go (docs/tools/selection.md): a click picks, the next starts.
    assert!(e.selection().is_empty());
    let at = px(&e, DVec3::new(300.0, 50.0, 50.0));
    e.input(ToolInput::Hover { px: at });
    e.input(ToolInput::Click { px: at, mods: Modifiers::default() });
    assert_eq!(e.selection(), &[a]);
    e.input(ToolInput::Hover { px: at });
    e.input(ToolInput::Click { px: at, mods: Modifiers::default() });
    tap(&mut e);
    assert!(e.tool_copying());
    e.input(ToolInput::Key { key: Key::Up, mods: Modifiers::default() });
    e.input(ToolInput::Vcb("100".into()));
    assert!(!e.tool_copying(), "back to moving after a copy");
    assert_eq!(e.document().root_assembly().occurrences().len(), 2);
    assert!(e.selection().is_empty(), "the copy let go");
    let copy = e.document().root_assembly().occurrences().iter().map(|o| o.id).find(|id| *id != a).unwrap();
    assert_ne!(copy, a);
    let (lo, _) = bounds_of(&e, &EntityPath::root(copy));
    // Up locks Z; in a top view the cursor's side is undefined, the distance is what counts.
    let dz = lo.z;
    assert!((dz.abs() - 100.0).abs() < 1e-6, "{lo}");

    // `3x`: an array of 3 at the same step, in place of the copy; retyped `/4`: 4 copies
    // spread over the step; both one undo step.
    let steps = e.history().journal().count();
    e.input(ToolInput::Vcb("3x".into()));
    assert_eq!(e.document().root_assembly().occurrences().len(), 4);
    assert_eq!(e.selection().len(), 3);
    let top =
        e.selection().iter().map(|id| bounds_of(&e, &EntityPath::root(*id)).0.z).fold(0.0f64, |m, z| if z.abs() > m.abs() { z } else { m });
    assert!((top - 3.0 * dz).abs() < 1e-6, "{top}");
    e.input(ToolInput::Vcb("/4".into()));
    assert_eq!(e.document().root_assembly().occurrences().len(), 5);
    let mut zs: Vec<f64> = e.selection().iter().map(|id| bounds_of(&e, &EntityPath::root(*id)).0.z.abs()).collect();
    zs.sort_by(f64::total_cmp);
    assert!(zs.iter().zip([25.0, 50.0, 75.0, 100.0]).all(|(z, w)| (z - w).abs() < 1e-6), "{zs:?}");
    assert_eq!(e.history().journal().count(), steps);
    e.undo();
    assert_eq!(e.document().root_assembly().occurrences().len(), 1);
}

#[test]
fn explode_is_one_action_by_what_is_selected() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "50");
    // A part alone: nothing to explode.
    e.select(vec![a]);
    assert!(!e.can_explode());
    // An assembly: its content put back in the context.
    e.make_assembly();
    assert!(e.can_explode());
    e.explode();
    assert!(e.document().assembly(e.document().root()).unwrap().occurrences().iter().all(|o| e.document().part(o.component).is_ok()));
    assert!(!e.can_explode());
}

#[test]
fn hide_rest_of_model() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "50");
    draw_box(&mut e, DVec3::new(300.0, 0.0, 0.0), DVec3::new(400.0, 100.0, 0.0), "50");
    e.select(vec![a]);
    e.make_assembly();
    let g = e.selection()[0];
    let mut s = e.document().settings().clone();
    s.display.hide_rest_of_model = true;
    e.apply(Operation::SetSettings(s)).unwrap();
    let drawn = |e: &Editor| e.view().batches.iter().map(|(_, p)| p.len()).sum::<usize>();
    let ray = |e: &Editor| e.camera.ray(px(e, DVec3::new(350.0, 50.0, 50.0)), e.viewport_size());
    // At the model's level, nothing is hidden.
    assert_eq!(drawn(&e), 2);
    // Editing the group: the other box is neither drawn, picked, inferred on nor framed.
    e.enter(g);
    assert_eq!(drawn(&e), 1);
    assert!(e.view().batches.iter().flat_map(|(_, p)| p).all(|p| !p.faded));
    assert!(e.scene().pick(&ray(&e), |_| true).is_none());
    assert!(e.scene().pick_inferable(&ray(&e), |_| true).is_none());
    assert!(e.scene().bounds().unwrap().1.x < 200.0);
    // Its part open: the same.
    e.enter(a);
    assert_eq!(e.open_part(), Some(a));
    assert_eq!(drawn(&e), 1);
    assert!(e.scene().pick(&ray(&e), |_| true).is_none());
    // Back at the model's level, it is all there again.
    e.exit();
    e.exit();
    assert_eq!(drawn(&e), 2);
    assert!(e.scene().pick(&ray(&e), |_| true).is_some());
    assert!(e.scene().bounds().unwrap().1.x > 300.0);
}

#[test]
fn select_model_leaves_every_context() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "50");
    draw_box(&mut e, DVec3::new(300.0, 0.0, 0.0), DVec3::new(400.0, 100.0, 0.0), "50");
    e.select(vec![a]);
    assert!(!e.model_chosen());
    e.make_assembly();
    let g = e.selection()[0];
    // At the model's level, something selected: only deselected.
    assert!(!e.model_chosen());
    e.select_model();
    assert!(e.selection().is_empty());
    assert!(e.model_chosen());
    // Deep in it: the assembly edited, its part open.
    e.enter(g);
    e.enter(a);
    assert_eq!(e.open_part(), Some(a));
    assert!(!e.model_chosen());
    e.select_model();
    assert!(e.context().is_empty());
    assert_eq!(e.open_part(), None);
    assert!(e.selection().is_empty());
    assert!(e.model_chosen());
    // Something else chosen, then nothing: the model is not chosen again by itself.
    e.select(vec![g]);
    e.poll();
    e.select(Vec::new());
    e.poll();
    assert!(!e.model_chosen());
    // A click in the view (in the void) chooses something else too.
    e.select_model();
    click(&mut e, DVec3::new(5000.0, 5000.0, 0.0));
    assert!(e.selection().is_empty());
    assert!(!e.model_chosen());
}

#[test]
fn outliner_hover_marks_in_view() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "50");
    let b = draw_box(&mut e, DVec3::new(300.0, 0.0, 0.0), DVec3::new(400.0, 100.0, 0.0), "50");
    e.select(vec![a]);
    e.make_assembly();
    let g = e.selection()[0];
    e.select(vec![]);
    // The cursor over the void: nothing hovered in the view.
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(-800.0, -800.0, 0.0)) });
    let hovered = |e: &Editor| e.view().highlights.into_iter().filter(|h| h.mark == Mark::Hover).count();
    assert_eq!(hovered(&e), 0);
    // An occurrence of the context: its solid, as if hovered in the view; changed once.
    assert!(e.set_outliner_hover(Some(EntityPath::new(vec![], b))));
    assert!(!e.set_outliner_hover(Some(EntityPath::new(vec![], b))));
    assert_eq!(hovered(&e), 1);
    // One inside an assembly not entered, too.
    e.set_outliner_hover(Some(EntityPath::new(vec![g], a)));
    assert_eq!(hovered(&e), 1);
    // Selected: marked so only.
    e.select(vec![g]);
    e.set_outliner_hover(Some(EntityPath::new(vec![], g)));
    assert_eq!(hovered(&e), 0);
    assert!(e.set_outliner_hover(None));
    assert_eq!(hovered(&e), 0);
    // Inside `g`: `b`, outside it, hovered but still faded; `a`, inside, plainly hovered.
    e.enter(g);
    let marks = |e: &Editor| {
        e.view().highlights.into_iter().map(|h| h.mark).filter(|m| matches!(m, Mark::Hover | Mark::HoverFaded)).collect::<Vec<_>>()
    };
    e.set_outliner_hover(Some(EntityPath::new(vec![], b)));
    assert_eq!(marks(&e), [Mark::HoverFaded]);
    e.set_outliner_hover(Some(EntityPath::new(vec![g], a)));
    assert_eq!(marks(&e), [Mark::Hover]);
}

#[test]
fn hidden_occurrences() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "50");
    let b = draw_box(&mut e, DVec3::new(300.0, 0.0, 0.0), DVec3::new(400.0, 100.0, 0.0), "50");
    e.select(vec![b]);
    e.make_assembly();
    let g = e.selection()[0];
    let drawn = |e: &Editor| e.view().batches.iter().map(|(_, p)| p.len()).sum::<usize>();
    let faded = |e: &Editor| e.view().batches.iter().flat_map(|(_, p)| p).filter(|p| p.faded).count();
    let ray = |e: &Editor| e.camera.ray(px(e, DVec3::new(350.0, 50.0, 50.0)), e.viewport_size());
    assert!(!e.has_hidden());

    // The assembly hidden: neither drawn, picked, inferred on nor framed; out of the selection.
    e.select(vec![a, g]);
    e.set_hidden(&EntityPath::root(g), true);
    assert!(e.has_hidden());
    assert_eq!(e.selection(), [a]);
    assert_eq!(drawn(&e), 1);
    assert!(e.scene().pick(&ray(&e), |_| true).is_none());
    assert!(e.scene().pick_inferable(&ray(&e), |_| true).is_none());
    assert!(e.scene().bounds().unwrap().1.x < 200.0);
    e.select_all();
    assert_eq!(e.selection(), [a]);

    // Hovered in the outliner, it or its part: not marked.
    e.select(vec![]);
    let hovered = |e: &Editor| e.view().highlights.into_iter().filter(|h| matches!(h.mark, Mark::Hover | Mark::HoverFaded)).count();
    e.set_outliner_hover(Some(EntityPath::root(g)));
    assert_eq!(hovered(&e), 0);
    e.set_outliner_hover(Some(EntityPath::new(vec![g], b)));
    assert_eq!(hovered(&e), 0);
    e.set_outliner_hover(Some(EntityPath::root(a)));
    assert_eq!(hovered(&e), 1);
    e.set_outliner_hover(None);

    // Edited, it shows (its part inside, not hidden itself); back out, hidden again.
    e.enter(g);
    assert_eq!((drawn(&e), faded(&e)), (2, 1));
    assert!(e.scene().pick(&ray(&e), |_| true).is_some());
    e.set_outliner_hover(Some(EntityPath::new(vec![g], b)));
    assert_eq!(hovered(&e), 1);
    e.set_outliner_hover(None);
    e.exit();
    assert_eq!(drawn(&e), 1);

    // With the hidden shown: drawn faded, picked and framed.
    let mut s = e.document().settings().clone();
    s.display.show_hidden = true;
    e.apply(Operation::SetSettings(s.clone())).unwrap();
    assert_eq!((drawn(&e), faded(&e)), (2, 1));
    assert!(e.scene().pick(&ray(&e), |_| true).is_some());
    assert!(e.scene().bounds().unwrap().1.x > 300.0);
    s.display.show_hidden = false;
    e.apply(Operation::SetSettings(s)).unwrap();

    // Undone (the settings changed in a row, one step; then the hiding), then all shown
    // again.
    e.undo();
    e.undo();
    assert!(e.document().root_assembly().occurrence(g).is_some());
    assert!(!e.has_hidden());
    assert_eq!((drawn(&e), faded(&e)), (2, 0));
    e.set_hidden(&EntityPath::root(a), true);
    assert_eq!(drawn(&e), 1);
    e.show_all();
    assert!(!e.has_hidden());
    assert_eq!(drawn(&e), 2);
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
    assert_eq!(e.document().assembly(gdef).unwrap().occurrences().len(), 2);
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
    let segs = boxes_in(&e.view(), ThemeColor::Selected);
    let (lo, hi) = segs.iter().flat_map(|b| b.corners()).fold((DVec3::MAX, DVec3::MIN), |(l, h), p| (l.min(p), h.max(p)));
    assert!(close(lo, DVec3::ZERO) && close(hi, DVec3::new(100.0, 100.0, 50.0)), "{lo} {hi}");
    e.select(vec![a, b]);
    e.make_assembly();
    let segs = boxes_in(&e.view(), ThemeColor::Selected);
    assert!(!segs.is_empty());
    // Dashed, the box spans the content's bounds exactly.
    let (lo, hi) = segs.iter().flat_map(|b| b.corners()).fold((DVec3::MAX, DVec3::MIN), |(l, h), p| (l.min(p), h.max(p)));
    assert!(close(lo, DVec3::ZERO) && close(hi, DVec3::new(400.0, 200.0, 80.0)), "{lo} {hi}");
    // The stretch tool draws its own box: the selection's is not drawn.
    e.set_tool(ToolKind::Stretch);
    assert!(boxes_in(&e.view(), ThemeColor::Selected).is_empty());
    e.set_tool(ToolKind::Select);
    assert!(!boxes_in(&e.view(), ThemeColor::Selected).is_empty());
    e.select(vec![]);
    assert!(boxes_in(&e.view(), ThemeColor::Selected).is_empty());
    assert!(boxes_in(&e.view(), ThemeColor::ContextBox).is_empty());
    // Inside the assembly: its box stays, a little larger than the content.
    let g = {
        e.select_all();
        e.selection()[0]
    };
    e.enter(g);
    let v = e.view();
    assert!(boxes_in(&v, ThemeColor::Selected).is_empty());
    let (lo, hi) = boxes_in(&v, ThemeColor::ContextBox)
        .iter()
        .flat_map(|b| b.corners())
        .fold((DVec3::MAX, DVec3::MIN), |(l, h), p| (l.min(p), h.max(p)));
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
            material: None,
        })
        .unwrap();
    let (d, out) = d.apply(&Operation::MakeAssembly { context: root, entities: out.created, name: None }).unwrap();
    let i1 = out.created[0];
    let asm = d.resolve(&EntityPath::root(i1)).unwrap().component;
    // Meshes are per part: the assembly's one part.
    let def = d.assembly(asm).unwrap().occurrences()[0].component;
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
            material: None,
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
    assert_eq!(part.machinings().len(), 1);
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
    // The box just drawn is selected: it is what turns (docs/tools/selection.md).
    assert_eq!(e.selection(), &[a]);
    // Centre on the box's top face (axis: its normal, Z), reference along +X, then 90° typed.
    // The cursor a hair inside the top face (the corner itself is on three faces: which one a
    // ray exactly through it hits is a tie), the centre snapped to the corner.
    click(&mut e, DVec3::new(1.0, 1.0, 50.0));
    assert!(e.preview().drawing.lines.len() > 40, "protractor drawn");
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
    assert!(e.selection().is_empty(), "turned, the selection is let go");
    e.input(ToolInput::Click { px: at, mods: Modifiers::default() });
    e.input(ToolInput::Hover { px: at });
    e.input(ToolInput::Click { px: at, mods: Modifiers::default() });
    click(&mut e, DVec3::new(-50.0 + 100.0, 100.0, 50.0));
    click(&mut e, DVec3::new(-50.0 + 100.0, 200.0, 50.0));
    assert_eq!(e.document().root_assembly().occurrences().len(), 2);

    // `/2` right after: two copies spread over the angle (the last one where the copy was).
    let copy = e.document().root_assembly().occurrences().iter().map(|o| o.id).find(|id| *id != a).unwrap();
    let before = bounds_of(&e, &EntityPath::root(copy));
    e.input(ToolInput::Vcb("/2".into()));
    assert_eq!(e.document().root_assembly().occurrences().len(), 3);
    let last = *e.selection().last().unwrap();
    let after = bounds_of(&e, &EntityPath::root(last));
    assert!(close(before.0, after.0) && close(before.1, after.1), "{before:?} {after:?}");
}

#[test]
fn rotate_snaps_to_the_graduations_inside_the_protractor_only() {
    let mut e = top_editor(Document::new());
    draw_box(&mut e, DVec3::new(-600.0, -600.0, 0.0), DVec3::new(-500.0, -500.0, 0.0), "20");
    e.set_tool(ToolKind::Rotate);
    let r = e.camera.world_per_pixel(e.viewport_size(), DVec3::ZERO) * 70.0;
    let at = |deg: f64, d: f64| DVec3::new(deg.to_radians().cos(), deg.to_radians().sin(), 0.0) * d;
    let hover = |e: &mut Editor, p: DVec3| e.input(ToolInput::Hover { px: px(e, p) });
    // The radius to the cursor: the last line drawn.
    let radius_end = |e: &Editor| e.preview().drawing.lines.last().unwrap().b;
    click(&mut e, DVec3::ZERO);

    // The reference, inside: on the graduation nearest (the context's X), up to it.
    hover(&mut e, at(5.0, r * 0.5));
    assert!(close(radius_end(&e), DVec3::new(r, 0.0, 0.0)), "{}", radius_end(&e));
    click(&mut e, at(5.0, r * 0.5));

    // The angle, inside: in steps of 15° from the reference.
    hover(&mut e, at(50.0, r * 0.6));
    assert_eq!(e.vcb().map(|v| v.1), Some("45°".into()));
    assert!(close(radius_end(&e), at(45.0, r)), "{}", radius_end(&e));
    // Past it: no steps, the radius to the cursor.
    hover(&mut e, at(50.0, r * 3.0));
    assert_eq!(e.vcb().map(|v| v.1), Some("50°".into()));
    assert!(close(radius_end(&e), at(50.0, r * 3.0)), "{}", radius_end(&e));
    // ... but the axes still snap it (the radius to the cursor projected on the axis).
    hover(&mut e, at(89.5, r * 3.0));
    assert_eq!(e.vcb().map(|v| v.1), Some("90°".into()));
    assert!(close(radius_end(&e), DVec3::new(0.0, r * 3.0 * 0.5f64.to_radians().cos(), 0.0)), "{}", radius_end(&e));
}

// ---- the kernel off the UI thread (docs/model/kernel.md) ----------------------------------

mod deferred {
    use std::cell::RefCell;
    use std::rc::Rc;

    use skraft_kernel::{Error, Job, JobId, JobResult, KernelService};

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
            self.results.borrow_mut().extend(jobs.into_iter().map(|(id, job)| (id, skraft_kernel::run(&job))));
        }

        /// Runs the first `n` jobs queued (in the order they were submitted).
        pub fn run_first(&self, n: usize) {
            let jobs: Vec<_> = {
                let mut q = self.jobs.borrow_mut();
                let n = n.min(q.len());
                q.drain(..n).collect()
            };
            self.results.borrow_mut().extend(jobs.into_iter().map(|(id, job)| (id, skraft_kernel::run(&job))));
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

        fn cancel(&mut self, id: JobId) {
            let mut jobs = self.jobs.borrow_mut();
            if let Some(i) = jobs.iter().position(|(j, _)| *j == id) {
                jobs.remove(i);
                self.results.borrow_mut().push((id, Err(Error::Cancelled)));
            }
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
        glam::DMat3::IDENTITY,
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
        material: None,
    };
    assert_eq!(e.apply(op), Ok(None), "waits for the kernel");
    k.run_all();
    assert!(e.poll());
    e.document().root_assembly().occurrences()[0].id
}

#[test]
fn pending_operation_blocks_editing_until_its_result() {
    let (mut e, k) = deferred_editor(Document::new());
    let root = e.document().root();
    let op = Operation::AddPrimitive {
        context: root,
        primitive: Primitive::Box { origin: DVec3::ZERO, size: DVec3::splat(100.0) },
        placement: glam::DAffine3::IDENTITY,
        material: None,
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
    assert!(e.document().root_assembly().occurrences().is_empty());

    k.run_all();
    assert!(e.poll());
    assert!(!e.busy());
    assert_eq!(e.document().root_assembly().occurrences().len(), 1);
    // The result came with its mesh: drawn and pickable, nothing more asked.
    assert_eq!(k.queued(), 0);
    let hit = e.scene().pick(&e.camera.ray(px(&e, DVec3::new(50.0, 50.0, 100.0)), e.viewport_size()), |_| true);
    assert!(hit.is_some());
}

/// What the editor told the app's toasts since last asked: titles and details, on lines.
fn notices(e: &mut Editor) -> String {
    e.take_notices().into_iter().map(|n| format!("{} {}", n.title, n.detail.unwrap_or_default())).collect::<Vec<_>>().join("\n")
}

#[test]
fn escape_abandons_and_failures_leave_the_document() {
    let (mut e, k) = deferred_editor(Document::new());
    let root = e.document().root();
    let op = Operation::AddPrimitive {
        context: root,
        primitive: Primitive::Box { origin: DVec3::ZERO, size: DVec3::splat(100.0) },
        placement: glam::DAffine3::IDENTITY,
        material: None,
    };
    assert_eq!(e.apply(op.clone()), Ok(None));
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    assert!(!e.busy());
    let said = notices(&mut e);
    assert!(said.contains("abandonné"), "{said}");
    k.run_all();
    e.poll();
    assert!(e.document().root_assembly().occurrences().is_empty(), "the late result is dropped");

    assert_eq!(e.apply(op), Ok(None));
    k.crash_all();
    e.poll();
    assert!(!e.busy());
    assert!(e.document().root_assembly().occurrences().is_empty());
    let said = notices(&mut e);
    assert!(said.contains("a échoué") && said.contains("noyau"), "{said}");
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
    assert_eq!(e.document().root_assembly().occurrences().len(), 2);

    // `5x` typed after the copy: the copy is replaced by the array, at once, in one step.
    e.run(Command::Replace {
        previous: Box::new(copy.clone()),
        op: Box::new(Operation::Array { context: root, entities: vec![s], step: t, count: 5 }),
    });
    assert!(!e.busy());
    assert_eq!(e.document().root_assembly().occurrences().len(), 6);
    assert_eq!(k.queued(), 0, "nothing to mesh: one part");
    e.undo();
    assert_eq!(e.document().root_assembly().occurrences().len(), 1);
    e.redo();

    // A refused array leaves the copy as it was.
    e.undo();
    e.apply(copy.clone()).unwrap();
    e.run(Command::Replace {
        previous: Box::new(copy.clone()),
        op: Box::new(Operation::Array { context: root, entities: vec![s], step: t, count: 0 }),
    });
    assert_eq!(e.history().undo_op(), Some(&copy));
    assert_eq!(e.document().root_assembly().occurrences().len(), 2);
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
            material: None,
        })
        .unwrap();
    let (mut e, k) = deferred_editor(doc);
    // Outlined: twelve edges, no faces; its meshes asked for once (opened: a preview, then
    // the final one).
    let part = &e.scene().definition_meshes().next().unwrap().parts[0];
    assert!(part.mesh.indices.is_empty());
    assert_eq!(part.mesh.edges.len(), 12);
    assert_eq!(k.queued(), 2);
    e.zoom_extents(); // the outline has bounds: framing works
    e.input(ToolInput::Hover { px: DVec2::new(500.0, 500.0) });
    assert_eq!(k.queued(), 2, "asked once");
    k.run_all();
    assert!(e.poll());
    assert!(!e.scene().definition_meshes().next().unwrap().parts[0].mesh.indices.is_empty());

    // Move the part's occurrence (no kernel, the mesh stays), then undo / redo: no meshing.
    let s = e.document().root_assembly().occurrences()[0].id;
    let t = glam::DAffine3::from_translation(DVec3::new(10.0, 0.0, 0.0));
    assert!(matches!(e.apply(Operation::Transform { context: root, entities: vec![s], transform: t }), Ok(Some(_))));
    assert_eq!(k.queued(), 0);
    e.undo();
    e.redo();
    e.undo();
    assert_eq!(k.queued(), 0, "meshes kept for the history");
    assert!(!e.scene().definition_meshes().next().unwrap().parts[0].mesh.indices.is_empty());
}

/// An opened file is a load (docs/ui/loading.md): every solid's preview asked before
/// any final mesh, drawn as it comes, the load counting both until the last final mesh; or
/// cancelled, its jobs stopped.
#[test]
fn an_opened_file_loads_previews_then_final_meshes() {
    let doc = Document::new();
    let root = doc.root();
    let add = |doc: Document, x: f64| {
        doc.apply(&Operation::AddPrimitive {
            context: root,
            primitive: Primitive::Box { origin: DVec3::new(x, 0.0, 0.0), size: DVec3::splat(100.0) },
            placement: glam::DAffine3::IDENTITY,
            material: None,
        })
        .unwrap()
        .0
    };
    let doc = add(add(doc, 0.0), 300.0);
    let (mut e, k) = deferred_editor(doc.clone());
    let loading = e.loading().cloned().unwrap();
    assert_eq!(loading.kind, LoadKind::Open);
    assert_eq!((loading.previews, loading.finals), ((0, 2), (0, 2)));
    assert_eq!(k.queued(), 4);
    // The previews first: drawn, the load going on.
    k.run_first(2);
    assert!(e.poll());
    assert_eq!(e.loading().unwrap().previews, (2, 2));
    assert!(e.scene().definition_meshes().all(|m| !m.parts[0].mesh.indices.is_empty()));
    assert_eq!(e.take_load_end(), None);
    // Then the final meshes: the load is over.
    k.run_all();
    assert!(e.poll());
    assert!(e.loading().is_none());
    assert_eq!(e.take_load_end(), Some(LoadEnd::Done));
    assert_eq!(e.take_load_end(), None, "taken once");

    // Cancelled: its jobs stopped, the app told.
    let (mut e, k) = deferred_editor(doc);
    e.cancel_loading();
    assert_eq!(k.queued(), 0);
    assert!(e.loading().is_none());
    assert_eq!(e.take_load_end(), Some(LoadEnd::Cancelled(LoadKind::Open)));
    e.poll(); // the cancelled jobs' answers: dropped
    assert!(e.loading().is_none());
}

/// A STEP file imported is a load: read, inserted, its meshes; cancelled while read, nothing
/// comes in; cancelled once inserted, the insertion is undone.
#[test]
fn a_step_import_is_a_load_that_can_be_cancelled() {
    let bytes = include_bytes!("../../kernel/tests/fixtures/assembly.step").to_vec();
    let (mut e, k) = deferred_editor(Document::new());
    e.import("assembly.step".into(), bytes.clone());
    let loading = e.loading().cloned().unwrap();
    assert_eq!(loading.kind, LoadKind::Import { name: "assembly.step".into() });
    assert_eq!(loading.reading, Some(skraft_kernel::Progress::Reading));
    // Read and inserted: its meshes asked as part of the load.
    k.run_all();
    assert!(e.poll());
    assert_eq!(e.document().root_assembly().occurrences().len(), 1);
    let loading = e.loading().cloned().unwrap();
    assert_eq!(loading.reading, None);
    // One solid (the board, twice placed): its preview and its final mesh.
    assert_eq!((loading.previews, loading.finals), ((0, 1), (0, 1)));
    k.run_all();
    e.poll();
    assert!(e.loading().is_none());
    assert_eq!(e.take_load_end(), Some(LoadEnd::Done));

    // Cancelled while read: nothing comes in.
    let (mut e, k) = deferred_editor(Document::new());
    e.import("assembly.step".into(), bytes.clone());
    e.cancel_loading();
    assert_eq!(k.queued(), 0);
    e.poll();
    assert!(e.document().root_assembly().occurrences().is_empty());
    assert!(!e.is_importing());
    assert_eq!(e.take_load_end(), Some(LoadEnd::Cancelled(LoadKind::Import { name: "assembly.step".into() })));
    let said = notices(&mut e);
    assert!(said.contains("annulé"), "{said}");

    // Cancelled once inserted: undone.
    let (mut e, k) = deferred_editor(Document::new());
    e.import("assembly.step".into(), bytes);
    k.run_all();
    e.poll();
    assert_eq!(e.document().root_assembly().occurrences().len(), 1);
    e.cancel_loading();
    assert!(e.document().root_assembly().occurrences().is_empty());
    assert_eq!(k.queued(), 0);
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
    let mut e = Editor::new(Document::new(), Box::new(skraft_kernel::Inline::default()));
    e.crash_kernel();
    let s = e.status();
    assert!(s.contains("Plantage provoqué") && s.contains("plantage voulu"), "{s}");
    assert!(s.contains("la tâche suivante a abouti"), "{s}");
    assert!(!e.busy());
}

// ---- the tool's option bar (docs/ui/interface.md) and the scope of a change (docs/model/data-model.md) --------

#[test]
fn boolean_scope_option_counts_marks_and_cuts_one_or_all() {
    let doc = Document::new();
    let root = doc.root();
    let board = Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 100.0, 50.0) };
    let (d, out) = doc
        .apply(&Operation::AddPrimitive { context: root, primitive: board, placement: glam::DAffine3::IDENTITY, material: None })
        .unwrap();
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
            material: None,
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
    assert_eq!(doc.resolve_part(&EntityPath::root(b)).unwrap().1.machinings().len(), 1);
    assert!(doc.resolve_part(&EntityPath::root(a)).unwrap().1.machinings().is_empty());
    // The middle board, a part of its own now, placed once: no scope to choose; nothing aimed
    // at, the scope back.
    // The status bar says Tab only while it switches something.
    hover(&mut e, DVec3::new(20.0, 320.0, 50.0));
    assert!(e.tool_options().iter().all(|o| o.key != "scope"));
    assert!(!e.status().contains("Tab"), "{}", e.status());
    hover(&mut e, DVec3::new(-2000.0, -2000.0, 0.0));
    assert_eq!(scope(&e).choices[0].label, "Tous les exemplaires");
    assert!(e.status().ends_with(" · Tab : Portée → Tous les exemplaires"), "{}", e.status());

    // Back to all, by a click in the bar: the other two both get the cut.
    e.set_tool_option("scope", OptionValue::Choice("all".into()));
    let d2 = drill(&mut e, 0.0);
    e.set_tool(ToolKind::Boolean(Boolean::Difference));
    click(&mut e, DVec3::new(20.0, 20.0, 50.0));
    click(&mut e, DVec3::new(100.0, 50.0, 50.0));
    let doc = e.document();
    assert!(doc.root_assembly().occurrence(d2).is_none(), "{}", e.status());
    assert_eq!(part(doc, c), part(doc, a));
    assert_eq!(doc.resolve_part(&EntityPath::root(c)).unwrap().1.machinings().len(), 1);

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

/// The paint tool shows what a click would paint in the colour it would take, and its pipette
/// takes the material shown under the cursor (docs/render/look.md).
#[test]
fn paint_tool_previews_its_colour_and_picks_with_its_pipette() {
    use skraft_i18n::t;
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "50");
    let b = draw_box(&mut e, DVec3::new(300.0, 0.0, 0.0), DVec3::new(400.0, 100.0, 0.0), "50");
    // Never painted.
    draw_box(&mut e, DVec3::new(600.0, 0.0, 0.0), DVec3::new(700.0, 100.0, 0.0), "50");
    e.apply(Operation::AddMaterial { name: "Chêne".into(), color: [180, 140, 90] }).unwrap();
    e.apply(Operation::AddMaterial { name: "Noyer".into(), color: [90, 60, 40] }).unwrap();
    let ids: Vec<Id> = e.document().materials().map(|(id, _)| id).collect();
    let (oak, walnut) = (ids[0], ids[1]);
    let (top_a, top_b, top_c) = (DVec3::new(50.0, 50.0, 50.0), DVec3::new(350.0, 50.0, 50.0), DVec3::new(650.0, 50.0, 50.0));
    let painted = |e: &Editor| {
        let mut marks: Vec<(Option<Id>, Option<u32>)> =
            e.view().highlights.iter().filter(|h| h.mark == Mark::Painted).map(|h| (h.material, h.face)).collect();
        marks.sort();
        marks
    };
    e.set_tool(ToolKind::Paint);
    e.set_paint_material(Some(walnut));
    click(&mut e, top_b);
    e.set_paint_material(Some(oak));

    // Hovered, an occurrence shows the colour it would take; none, the colour it would have.
    hover(&mut e, top_a);
    assert_eq!(painted(&e), [(Some(oak), None)]);
    e.set_paint_material(None);
    hover(&mut e, top_b);
    assert_eq!(painted(&e), [(None, None)], "unpainted: the default colour");
    e.set_paint_material(Some(oak));

    // A face: alone, in the colour it would take, over the part's.
    e.set_tool_option("target", OptionValue::Choice("face".into()));
    hover(&mut e, top_b);
    let marks = painted(&e);
    assert!(matches!(marks.as_slice(), [(Some(m), Some(_))] if *m == oak), "{marks:?}");
    click(&mut e, top_b);
    e.set_paint_material(None);
    hover(&mut e, top_b);
    let marks = painted(&e);
    assert!(matches!(marks.as_slice(), [(Some(m), Some(_))] if *m == walnut), "unpainted, the occurrence's: {marks:?}");

    // « Prélever » on its own: its tooltip names the material under the cursor — the face's
    // own, else the occurrence's, else none —; a click takes it, then the paint tool starts.
    e.set_tool(ToolKind::Sample);
    assert_eq!(e.tool_cursor(), Some(Cursor::Picker));
    hover(&mut e, top_b);
    assert_eq!(e.inference_tip().as_deref(), Some("Chêne"));
    assert!(painted(&e).is_empty(), "it paints nothing");
    hover(&mut e, top_c);
    assert_eq!(e.inference_tip(), Some(t!("panel.materials.none")));
    click(&mut e, top_b);
    assert_eq!((e.paint_material(), e.tool_kind()), (Some(oak), ToolKind::Paint));

    // Held over the paint tool by the other modifier (the tool stack): each click takes, the
    // paint tool waiting under it as it was, back when the key is released.
    e.set_tool_option("target", OptionValue::Choice("occurrence".into()));
    e.set_alt(true);
    assert_eq!((e.tool_kind(), e.suspended_tools()), (ToolKind::Sample, vec![ToolKind::Paint]));
    assert_eq!(e.tool_cursor(), Some(Cursor::Picker));
    click(&mut e, top_c);
    assert_eq!(e.paint_material(), None);
    e.apply(Operation::PaintOccurrences { context: e.document().root(), entities: vec![a], material: Some(walnut) }).unwrap();
    click(&mut e, top_a);
    assert_eq!((e.paint_material(), e.tool_kind()), (Some(walnut), ToolKind::Sample), "held: it takes again");
    e.set_alt(false);
    assert_eq!((e.tool_kind(), e.suspended_tools()), (ToolKind::Paint, vec![]));
    assert_eq!(e.tool_cursor(), Some(Cursor::PaintPart));
    // Esc hands back too.
    e.set_alt(true);
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    assert_eq!((e.tool_kind(), e.suspended_tools()), (ToolKind::Paint, vec![]));
    e.set_alt(false);
    assert_eq!(e.tool_kind(), ToolKind::Paint);
    // Choosing a tool empties the stack.
    e.set_alt(true);
    e.set_tool(ToolKind::Select);
    assert!(e.suspended_tools().is_empty());
    e.set_alt(false);
    assert_eq!(e.tool_kind(), ToolKind::Select);
    e.set_tool(ToolKind::Paint);

    // An assembly: its parts in the colour they would inherit, a part painted keeping its own.
    let root = e.document().root();
    e.apply(Operation::PaintOccurrences { context: root, entities: vec![a], material: None }).unwrap();
    let asm = e.apply(Operation::MakeAssembly { context: root, entities: vec![a, b], name: None }).unwrap().unwrap().created[0];
    e.select(vec![]);
    e.set_paint_material(Some(oak));
    hover(&mut e, top_a);
    assert_eq!(painted(&e), [(Some(oak), None), (Some(walnut), None)], "b keeps its walnut");
    assert_eq!(e.view().highlights.iter().filter(|h| h.mark == Mark::Painted).count(), 2);
    click(&mut e, top_a);
    let occurrence = |e: &Editor| e.document().assembly(root).unwrap().occurrence(asm).unwrap().material;
    assert_eq!(occurrence(&e), Some(oak));
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

    // Its cut line: the box's outline at 70, once round; selected, drawn again marked.
    let length = |cuts: &[crate::SectionCut]| cuts.iter().map(|c| c.a.distance(c.b)).sum::<f64>();
    e.select(vec![]);
    let cuts = e.view().section_cuts;
    assert!(cuts.iter().all(|c| (c.a.z - 70.0).abs() < 1e-3 && (c.b.z - 70.0).abs() < 1e-3 && c.mark.is_none()), "{cuts:?}");
    assert!((length(&cuts) - 400.0).abs() < 1e-3, "{}", length(&cuts));
    e.select(vec![a]);
    let cuts = e.view().section_cuts;
    let marked: Vec<_> = cuts.iter().filter(|c| c.mark == Some(Mark::Selected)).copied().collect();
    assert!((length(&marked) - 400.0).abs() < 1e-3, "{}", length(&marked));

    // Undo removes it.
    e.undo();
    assert!(e.document().settings().display.active_section().is_none());
    assert!(e.view().section_cuts.is_empty());
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
    assert!(!boxes_in(&v, ThemeColor::ContextBox).is_empty());
    let top = boxes_in(&v, ThemeColor::ContextBox).iter().flat_map(|b| b.corners().map(|c| c.z)).fold(f64::MIN, f64::max);
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
    assert_eq!(part.machinings().len(), 1);
    let m = part.machinings()[0].clone();
    assert!(close(m.placement.translation, DVec3::new(100.0, 100.0, 30.0)), "{}", m.placement.translation);
    let (lo, hi) = part.solid_of(&m).vertex_bounds().unwrap();
    assert!(close(hi - lo, DVec3::new(150.0, 100.0, 20.0)), "{lo} {hi}");
    // Every occurrence shows it (scope "all"): `a` is the same part.
    assert_eq!(e.document().resolve_part(&EntityPath::root(a)).unwrap().1.machinings().len(), 1);

    // Esc closes it, the occurrence left selected; so does a click in the void.
    e.enter(b);
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    assert_eq!((e.open_part(), e.selection()), (None, &[b][..]));
    e.enter(b);
    click(&mut e, DVec3::new(-800.0, -800.0, 0.0));
    assert_eq!(e.open_part(), None);
    assert!(e.axes_transform().abs_diff_eq(DAffine3::IDENTITY, 1e-12));
    assert!(boxes_in(&e.view(), ThemeColor::ContextBox).is_empty());

    // Undo: the pocket goes, the part stays open; then the copy goes, and the part closes.
    e.enter(b);
    e.undo();
    assert_eq!(e.open_part(), Some(b));
    assert!(e.document().resolve_part(&EntityPath::root(b)).unwrap().1.machinings().is_empty());
    e.undo();
    assert_eq!(e.open_part(), None);
}

#[test]
fn an_open_part_shows_its_blank_as_a_ghost_and_its_tools() {
    use glam::DAffine3;
    use skraft_model::Scope;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let board = draw_box(&mut e, DVec3::ZERO, DVec3::new(400.0, 200.0, 0.0), "18");
    let drill = e
        .apply(Operation::AddPrimitive {
            context: root,
            primitive: Primitive::Cylinder { origin: DVec3::new(50.0, 100.0, -5.0), radius: 4.0, height: 30.0 },
            placement: DAffine3::IDENTITY,
            material: None,
        })
        .unwrap()
        .unwrap()
        .created[0];
    let target = EntityPath::root(board);
    e.apply(Operation::boolean(target.clone(), EntityPath::root(drill), Boolean::Difference, Scope::All)).unwrap();
    let (part, p, _) = e.document().resolve_part(&target).unwrap();
    let hole = p.machinings()[0].id;
    e.apply(Operation::DuplicateBodies {
        target: target.clone(),
        bodies: vec![hole],
        transform: DAffine3::from_translation(DVec3::X * 100.0),
        scope: Scope::All,
    })
    .unwrap();
    let looks = |e: &Editor| e.view().batches.iter().flat_map(|(k, ps)| ps.iter().map(move |p| (*k, p.look))).collect::<Vec<_>>();

    // No part open: shapes only.
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
    // Meshed like shapes (the kernel is inline here): the blank, a box, takes its 12 triangles.
    for (k, _) in blank.iter().chain(&tools) {
        assert!(e.scene().definition_mesh(*k).is_some_and(|m| m.parts[0].mesh.indices.len() >= 36));
    }
    // A tool placed in the world through the occurrence.
    let r = e.scene().recipe().unwrap();
    assert!(close(r.tools[1].placement.translation, DVec3::new(150.0, 100.0, -5.0)));
    // Each body's bottom cap drawn bold: the board's rectangle at z 0, each hole's circle at z -5.
    let bases: Vec<(DVec3, DVec3)> = bases_of(&e.view()).into_iter().map(|l| (l.a, l.b)).collect();
    let at_z = |z: f64| bases.iter().filter(|(a, b)| (a.z - z).abs() < 1e-6 && (b.z - z).abs() < 1e-6).collect::<Vec<_>>();
    assert_eq!(at_z(0.0).len(), 4, "{bases:?}");
    let holes = at_z(-5.0);
    assert_eq!(holes.len() + 4, bases.len());
    for x in [50.0, 150.0] {
        assert!(holes.iter().any(|(a, _)| ((a.truncate() - glam::DVec2::new(x, 100.0)).length() - 4.0).abs() < 1e-6));
    }
    // Hovered in the entity info's list: drawn hovered in the view.
    let copy = r.tools[1].body;
    e.set_listed_hover(Some(copy));
    assert_eq!(e.hovered_machining(), Some(copy));
    let hovered: Vec<_> = e.view().highlights.into_iter().filter(|h| h.mark == Mark::Hover).collect();
    assert_eq!(hovered.len(), 1);
    assert!(close(hovered[0].transform.translation, DVec3::new(150.0, 100.0, -5.0)));
    e.set_listed_hover(None);

    // Closed: the shape again.
    e.exit();
    assert!(looks(&e).iter().all(|(_, l)| *l == Look::Shape));
    assert!(bases_of(&e.view()).is_empty());
    assert!(e.scene().recipe().is_none());
}

#[test]
fn machinings_are_picked_moved_copied_arrayed_and_erased_in_the_open_part() {
    use glam::DAffine3;
    use skraft_model::Scope;
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
            material: None,
        })
        .unwrap()
        .unwrap()
        .created[0];
    let target = EntityPath::root(board);
    e.apply(Operation::boolean(target.clone(), EntityPath::root(drill), Boolean::Difference, Scope::All)).unwrap();
    let machinings = |e: &Editor, o: Id| e.document().resolve_part(&EntityPath::root(o)).unwrap().1.machinings().clone();
    let hole = machinings(&e, board)[0].id;
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

    // Move, nothing selected: the tool picked by a first click (docs/tools/selection.md), the start by
    // the next; 100 along red. While moving, the tool is a ghost where it goes, not drawn
    // where it was.
    e.set_tool(ToolKind::Move);
    click(&mut e, DVec3::new(50.0, 100.0, 25.0));
    assert_eq!(e.selection(), &[hole]);
    click(&mut e, DVec3::new(50.0, 100.0, 25.0));
    e.input(ToolInput::Key { key: Key::Right, mods: Modifiers::default() });
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(250.0, 100.0, 25.0)) });
    let v = e.view();
    assert!(v.highlights.iter().any(|h| h.mark == Mark::Ghost));
    assert!(!v.batches.iter().flat_map(|(_, p)| p).any(|p| p.look == Look::Tool));
    e.input(ToolInput::Vcb("100".into()));
    assert!(close(machinings(&e, board)[0].placement.translation, DVec3::new(150.0, 100.0, -5.0)));
    assert!(e.selection().is_empty(), "moved, let go");

    // A copy 50 further, then `3x`: an array of three, every occurrence shows them.
    e.input(ToolInput::Key { key: Key::Copy, mods: Modifiers::default() });
    click(&mut e, DVec3::new(150.0, 100.0, 25.0));
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

    // One of the four (one definition) moved: it alone drawn where it goes, not its copies.
    e.select(vec![ms[0].id]);
    let x0 = ms[0].placement.translation.x;
    click(&mut e, DVec3::new(x0, 100.0, 25.0));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(x0, 150.0, 25.0)) });
    assert_eq!(e.view().highlights.iter().filter(|h| h.mark == Mark::Ghost).count(), 1);
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });

    // Erased under "only this one": this occurrence gets a part of its own, the other keeps
    // the holes.
    e.set_tool(ToolKind::Select);
    // All: the blank (a positive body, docs/model/bodies.md) and the holes; the holes alone erased.
    e.select_all();
    assert_eq!(e.selection().len(), 5);
    e.select(machinings(&e, board).iter().map(|m| m.id).collect());
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
fn a_machining_moved_stays_drawn_where_it_goes_while_the_kernel_computes() {
    use glam::DAffine3;
    use skraft_model::Scope;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let board = draw_box(&mut e, DVec3::ZERO, DVec3::new(400.0, 200.0, 0.0), "18");
    let drill = e
        .apply(Operation::AddPrimitive {
            context: root,
            primitive: Primitive::Cylinder { origin: DVec3::new(50.0, 100.0, -5.0), radius: 4.0, height: 30.0 },
            placement: DAffine3::IDENTITY,
            material: None,
        })
        .unwrap()
        .unwrap()
        .created[0];
    let target = EntityPath::root(board);
    e.apply(Operation::boolean(target.clone(), EntityPath::root(drill), Boolean::Difference, Scope::All)).unwrap();
    let (mut e, k) = deferred_editor(e.document().clone());
    let settle = |e: &mut Editor| {
        while k.queued() > 0 {
            k.run_all();
            e.poll();
        }
    };
    settle(&mut e);
    e.enter(board);
    settle(&mut e);
    let hole = e.document().resolve_part(&target).unwrap().1.machinings()[0].id;
    let tools_drawn = |e: &Editor| e.view().batches.iter().flat_map(|(_, p)| p).filter(|p| p.look == Look::Tool).count();
    assert_eq!(tools_drawn(&e), 1);

    // Moved 100 along red, then copied 50 further: while the kernel works, the hole is drawn
    // where it goes (a ghost), not back where it was for a frame or more.
    for copy in [false, true] {
        e.set_tool(ToolKind::Move);
        e.select(vec![hole]);
        if copy {
            e.input(ToolInput::Key { key: Key::Copy, mods: Modifiers::default() });
        }
        let x = e.document().resolve_part(&target).unwrap().1.machinings()[0].placement.translation.x;
        click(&mut e, DVec3::new(x, 100.0, 25.0));
        e.input(ToolInput::Key { key: Key::Right, mods: Modifiers::default() });
        e.input(ToolInput::Hover { px: px(&e, DVec3::new(x + 200.0, 100.0, 25.0)) });
        e.input(ToolInput::Vcb("100".into()));
        assert!(e.busy(), "computed by the kernel");
        let v = e.view();
        assert_eq!(v.highlights.iter().filter(|h| h.mark == Mark::Ghost).count(), 1, "copy: {copy}");
        assert_eq!(tools_drawn(&e), usize::from(copy), "the original drawn only when copied");
        settle(&mut e);
        assert!(!e.busy());
        assert!(!e.view().highlights.iter().any(|h| h.mark == Mark::Ghost));
        assert_eq!(tools_drawn(&e), 1 + usize::from(copy));
    }
}

#[test]
fn inferences_see_the_blank_and_the_tools_while_machinings_show() {
    use glam::DAffine3;
    use skraft_model::Scope;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let board = draw_box(&mut e, DVec3::ZERO, DVec3::new(400.0, 200.0, 0.0), "18");
    // A drill standing out of the board's top: its top circle at z 25, in no shape.
    let drill = e
        .apply(Operation::AddPrimitive {
            context: root,
            primitive: Primitive::Cylinder { origin: DVec3::new(50.0, 100.0, -5.0), radius: 4.0, height: 30.0 },
            placement: DAffine3::IDENTITY,
            material: None,
        })
        .unwrap()
        .unwrap()
        .created[0];
    e.apply(Operation::boolean(EntityPath::root(board), EntityPath::root(drill), Boolean::Difference, Scope::All)).unwrap();
    let hover = |e: &mut Editor, p: DVec3| {
        let at = px(e, p);
        e.input(ToolInput::Hover { px: at });
        e.inference()
    };
    let centre = DVec3::new(50.0, 100.0, 25.0);
    e.set_tool(ToolKind::Move);

    // Closed: the shape only (the hole's rim, at z 18).
    let (p, _) = hover(&mut e, centre).unwrap();
    assert!(p.z < 18.0 + 1e-6, "{p}");

    // The part open, its bodies shown: the tool's top circle, its centre inferred.
    // Picked first (docs/tools/selection.md).
    e.enter(board);
    e.set_tool(ToolKind::Move);
    let at = px(&e, centre);
    e.input(ToolInput::Hover { px: at });
    e.input(ToolInput::Click { px: at, mods: Modifiers::default() });
    assert_eq!(e.selection().len(), 1);
    let (p, kind) = hover(&mut e, centre).unwrap();
    assert_eq!(kind, InferKind::Center);
    assert!(close(p, centre), "{p}");

    // Moving it from there: it is left out, the blank's top is what is under the cursor.
    e.input(ToolInput::Click { px: at, mods: Modifiers::default() });
    let (p, _) = hover(&mut e, DVec3::new(52.0, 100.0, 25.0)).unwrap();
    assert!(p.z < 18.0 + 1e-6, "{p}");
}

#[test]
fn exact_sizes_of_tools_and_blanks() {
    use glam::DAffine3;
    use skraft_model::Scope;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let rod = e
        .apply(Operation::AddPrimitive {
            context: root,
            primitive: Primitive::Cylinder { origin: DVec3::ZERO, radius: 20.0, height: 300.0 },
            placement: DAffine3::IDENTITY,
            material: None,
        })
        .unwrap()
        .unwrap()
        .created[0];
    let drill = e
        .apply(Operation::AddPrimitive {
            context: root,
            primitive: Primitive::Cylinder { origin: DVec3::new(0.0, 0.0, -5.0), radius: 4.0, height: 30.0 },
            placement: DAffine3::IDENTITY,
            material: None,
        })
        .unwrap()
        .unwrap()
        .created[0];
    let target = EntityPath::root(rod);
    e.apply(Operation::boolean(target.clone(), EntityPath::root(drill), Boolean::Difference, Scope::All)).unwrap();
    let (_, p, _) = e.document().resolve_part(&target).unwrap();
    let (blank, tool) = (p.blank().clone(), p.solid_of(&p.machinings()[0]).clone());
    let near = |b: Option<(DVec3, DVec3)>, lo: DVec3, hi: DVec3| b.is_some_and(|(a, b)| close(a, lo) && close(b, hi));
    // A machined part's blank: its exact box, not its vertices' (a cylinder has one per circle).
    assert!(
        near(e.scene().bounds_of(&blank), DVec3::new(-20.0, -20.0, 0.0), DVec3::new(20.0, 20.0, 300.0)),
        "{:?}",
        e.scene().bounds_of(&blank)
    );
    // The open part's tools.
    e.enter(rod);
    assert!(e.showing_machinings());
    assert!(near(e.scene().bounds_of(&tool), DVec3::new(-4.0, -4.0, 0.0), DVec3::new(4.0, 4.0, 30.0)), "{:?}", e.scene().bounds_of(&tool));
}

#[test]
fn the_tape_tells_the_edge_hovered() {
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    draw_box(&mut e, DVec3::ZERO, DVec3::new(300.0, 200.0, 0.0), "100");
    let disc = Primitive::Cylinder { origin: DVec3::new(600.0, 600.0, 0.0), radius: 100.0, height: 50.0 };
    e.apply(Operation::AddPrimitive { context: root, primitive: disc, placement: glam::DAffine3::IDENTITY, material: None }).unwrap();
    let hover = |e: &mut Editor, p: DVec3| e.input(ToolInput::Hover { px: px(e, p) });
    e.set_tool(ToolKind::Measure);

    // A straight edge: its length.
    hover(&mut e, DVec3::new(300.0, 50.0, 100.0));
    assert_eq!(e.inference_tip(), Some("Longueur : 200".into()));
    // A circle: its length and its radius.
    let a = 45f64.to_radians();
    hover(&mut e, DVec3::new(600.0 + 100.0 * a.cos(), 600.0 + 100.0 * a.sin(), 50.0));
    assert_eq!(e.inference_tip(), Some("Longueur : ~ 628\nRayon : 100".into()));
    // A face: nothing.
    hover(&mut e, DVec3::new(150.0, 100.0, 100.0));
    assert_eq!(e.inference_tip(), None);
    // While measuring too.
    click(&mut e, DVec3::new(0.0, 0.0, 100.0));
    hover(&mut e, DVec3::new(300.0, 50.0, 100.0));
    assert_eq!(e.inference_tip(), Some("Longueur : 200".into()));
}

#[test]
fn the_tape_measures_and_draws_guides() {
    use skraft_model::GuideShape;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    draw_box(&mut e, DVec3::ZERO, DVec3::new(300.0, 200.0, 0.0), "100");
    let disc = Primitive::Cylinder { origin: DVec3::new(600.0, 600.0, 0.0), radius: 100.0, height: 50.0 };
    e.apply(Operation::AddPrimitive { context: root, primitive: disc, placement: glam::DAffine3::IDENTITY, material: None }).unwrap();
    let hover = |e: &mut Editor, p: DVec3| e.input(ToolInput::Hover { px: px(e, p) });
    let guides = |e: &Editor| e.document().component(root).unwrap().guides.iter().map(|g| g.shape).collect::<Vec<_>>();
    e.set_tool(ToolKind::Measure);
    assert!(e.vcb().is_none());
    // The saved kind of guide, said at once (no input yet).
    e.set_tool_option("guide", OptionValue::Choice("line".into()));
    e.set_tool(ToolKind::Select);
    e.set_tool(ToolKind::Measure);
    assert!(e.status().starts_with("Cliquer une arête pour un guide parallèle"), "{}", e.status());
    e.set_tool_option("guide", OptionValue::Choice("auto".into()));

    // From a corner to the opposite one: the length, and nothing added.
    click(&mut e, DVec3::new(0.0, 0.0, 100.0));
    hover(&mut e, DVec3::new(300.0, 200.0, 100.0));
    // √(300² + 200²) = 360,56: rounded.
    assert_eq!(e.vcb(), Some(("Longueur".into(), "~ 361".into())));
    assert_eq!(e.labels("").len(), 1, "the length written on the segment");
    let l = &e.labels("2*@")[0];
    assert_eq!((l.text.as_str(), l.result.as_deref(), l.active), ("2*@", Some("~ 721"), true), "typed: shown in it");
    click(&mut e, DVec3::new(300.0, 200.0, 100.0));
    assert!(e.vcb().is_none(), "the second click ends the measure");
    assert!(guides(&e).is_empty());

    // From an edge: a parallel guide through the point clicked next, the offset shown.
    click(&mut e, DVec3::new(300.0, 50.0, 100.0));
    hover(&mut e, DVec3::new(420.0, 120.0, 100.0));
    assert_eq!(e.vcb(), Some(("Décalage".into(), "120".into())));
    // The edge referred to, end to end, bolder.
    let refs: Vec<_> = e.preview().drawing.lines.into_iter().filter(|l| l.pen.color == crate::tools::MAGENTA.into()).collect();
    let (a, b) = (DVec3::new(300.0, 0.0, 100.0), DVec3::new(300.0, 200.0, 100.0));
    let [r] = refs[..] else { panic!("{refs:?}") };
    assert!(r.pen.width == crate::draw::BOLD && ((close(r.a, a) && close(r.b, b)) || (close(r.a, b) && close(r.b, a))), "{r:?}");
    // The offset runs from the point clicked, square to the edge, to the guide.
    let l = &e.labels("")[0];
    assert!(close(l.a, DVec3::new(300.0, 50.0, 100.0)) && close(l.b, DVec3::new(420.0, 50.0, 100.0)), "{} {}", l.a, l.b);
    // Along the red axis, whatever the cursor snaps to: red.
    assert_eq!(l.color, crate::tools::RED);
    click(&mut e, DVec3::new(420.0, 120.0, 100.0));
    let [GuideShape::Line { origin, dir }] = guides(&e)[..] else { panic!("{:?}", guides(&e)) };
    assert!((origin.x - 420.0).abs() < 1e-3 && (origin.z - 100.0).abs() < 1e-3 && dir.cross(DVec3::Y).length() < 1e-9, "{origin} {dir}");
    assert!(!guides_in(&e.view(), ThemeColor::Guide).is_empty(), "drawn");

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
    // The offset runs from the point clicked along its radius, wherever the cursor is around.
    hover(&mut e, c + DVec3::new(0.0, 250.0, 0.0));
    let l = &e.labels("")[0];
    assert!(close(l.a, c + radial * 100.0) && close(l.b, c + radial * 250.0), "{} {}", l.a, l.b);
    hover(&mut e, c + radial * 160.0);
    assert_eq!(e.vcb(), Some(("Décalage".into(), "60".into())));
    hover(&mut e, c + radial * 300.0);
    assert_eq!(e.labels("")[0].color, crate::tools::INK, "askew: ink");
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

    // The copy modifier sets no guide: an edge is then measured from.
    e.input(ToolInput::Key { key: Key::Copy, mods: Modifiers::default() });
    click(&mut e, DVec3::new(300.0, 50.0, 100.0));
    assert_eq!(e.vcb().map(|v| v.0), Some("Longueur".into()));
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    // Tapped again: back to the last kind chosen (Line here), an edge starts its guide.
    e.set_tool_option("guide", OptionValue::Choice("line".into()));
    e.input(ToolInput::Key { key: Key::Copy, mods: Modifiers::default() });
    assert_eq!(e.tool_options().iter().find(|o| o.key == "guide").map(|o| o.value.clone()), Some(OptionValue::Choice("none".into())));
    e.input(ToolInput::Key { key: Key::Copy, mods: Modifiers::default() });
    assert_eq!(e.tool_options().iter().find(|o| o.key == "guide").map(|o| o.value.clone()), Some(OptionValue::Choice("line".into())));
    click(&mut e, DVec3::new(300.0, 50.0, 100.0));
    assert_eq!(e.vcb().map(|v| v.0), Some("Décalage".into()), "{}", e.status());
}

#[test]
fn the_tape_draws_the_kind_of_guide_chosen() {
    use skraft_model::GuideShape;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    draw_box(&mut e, DVec3::ZERO, DVec3::new(300.0, 200.0, 0.0), "100");
    let disc = Primitive::Cylinder { origin: DVec3::new(600.0, 600.0, 0.0), radius: 100.0, height: 50.0 };
    e.apply(Operation::AddPrimitive { context: root, primitive: disc, placement: glam::DAffine3::IDENTITY, material: None }).unwrap();
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
    // The offset: from the point clicked along its radius, wherever the cursor is around.
    hover(&mut e, c + DVec3::X * 150.0);
    let xray = x_rays(&e.preview());
    assert_eq!(xray.len(), 1);
    let (a, b) = (xray[0].a, xray[0].b);
    assert!(close(a, c - DVec3::Y * 100.0) && (b.x - c.x).abs() < 1e-3 && b.y < a.y - 1.0, "{xray:?}");
    e.input(ToolInput::Vcb("0".into()));
    let GuideShape::Circle { center, radius, .. } = guides(&e)[4] else { panic!("{} {:?}", e.status(), guides(&e)) };
    assert!(close(center, c) && (radius - 100.0).abs() < 1e-6, "{radius}");
    // Centred on the corner clicked, level, the radius typed.
    click(&mut e, DVec3::new(300.0, 200.0, 100.0));
    assert_eq!(e.vcb().map(|v| v.0), Some("Rayon".into()));
    hover(&mut e, DVec3::new(380.0, 200.0, 100.0));
    assert_eq!(e.vcb().map(|v| v.1), Some("80".into()));
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
    use skraft_model::GuideShape;
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
    use skraft_model::{GuideShape, Scope};
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let add = |e: &mut Editor, primitive| {
        e.apply(Operation::AddPrimitive { context: root, primitive, placement: glam::DAffine3::IDENTITY, material: None })
            .unwrap()
            .unwrap()
            .created[0]
    };
    // A board with its corner (300, 200) cut round: an arc from (200, 200) to (300, 100) on top.
    let board = add(&mut e, Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(300.0, 200.0, 50.0) });
    let drill = add(&mut e, Primitive::Cylinder { origin: DVec3::new(300.0, 200.0, -10.0), radius: 100.0, height: 70.0 });
    let target = EntityPath::root(board);
    e.apply(Operation::boolean(target, EntityPath::root(drill), Boolean::Difference, Scope::All)).unwrap();
    e.select(vec![]);
    let guides = |e: &Editor| e.document().component(root).unwrap().guides.iter().map(|g| g.shape).collect::<Vec<_>>();
    // The arc referred to, as drawn: its own points, bolder, and no guide in its stead.
    let magenta = |e: &Editor| {
        let p = e.preview();
        let on_arc = |q: DVec3| ((q - DVec3::new(300.0, 200.0, 50.0)).length() - 100.0).abs() < 1e-6 && (q.z - 50.0).abs() < 1e-9;
        let arc = p
            .drawing
            .lines
            .iter()
            .filter(|l| l.pen.color == crate::tools::MAGENTA.into() && l.pen.width == crate::draw::BOLD)
            .collect::<Vec<_>>();
        let magenta_guide =
            |l: &crate::draw::Line| l.pen.color == crate::tools::MAGENTA.into() && l.pen.stipple == crate::draw::Stipple::LongDashes;
        !arc.is_empty() && arc.iter().all(|l| on_arc(l.a) && on_arc(l.b)) && !p.drawing.lines.iter().any(magenta_guide)
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
    e.set_tool_option("guide", OptionValue::Choice("auto".into()));
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
    use skraft_model::GuideShape;
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
    assert!(!guides_in(&v, ThemeColor::Guide).is_empty() && guides_in(&v, ThemeColor::FadedGuide).is_empty());
    // Closed, the part's guide is outside the context: faded.
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    e.set_tool(ToolKind::Select);
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    assert_eq!(e.open_part(), None);
    let v = e.view();
    assert!(guides_in(&v, ThemeColor::Guide).is_empty() && !guides_in(&v, ThemeColor::FadedGuide).is_empty());
}

fn hover(e: &mut Editor, p: DVec3) {
    let at = px(e, p);
    e.input(ToolInput::Hover { px: at });
}

#[test]
fn the_tape_starts_a_guide_from_a_guide() {
    use skraft_model::GuideShape;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let line = GuideShape::Line { origin: DVec3::new(0.0, -300.0, 0.0), dir: DVec3::X };
    e.apply(Operation::AddGuide { component: root, shape: line }).unwrap();
    let circle = GuideShape::Circle { center: DVec3::new(400.0, 400.0, 0.0), normal: DVec3::Z, radius: 150.0 };
    e.apply(Operation::AddGuide { component: root, shape: circle }).unwrap();
    e.set_tool(ToolKind::Measure);
    // The guide referred to, dashed where it is: its very strokes, in magenta.
    let marks_guide = |e: &Editor| {
        let magenta_guide =
            |l: &&crate::draw::Line| l.pen.color == crate::tools::MAGENTA.into() && l.pen.stipple == crate::draw::Stipple::LongDashes;
        let refs: Vec<_> = e.preview().drawing.lines.iter().filter(magenta_guide).map(|l| (l.a, l.b, l.phase)).collect();
        let drawn: Vec<_> = guides_in(&e.view(), ThemeColor::Guide).into_iter().map(|l| (l.a, l.b, l.phase)).collect();
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
    use skraft_model::GuideShape;
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
    assert!(!guides_in(&e.view(), ThemeColor::Hover).is_empty());
    click(&mut e, DVec3::new(100.0, -303.0, 0.0));
    assert_eq!(e.selection(), &[g]);
    assert!(!guides_in(&e.view(), ThemeColor::Selected).is_empty());
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
    assert!(guides_in(&v, ThemeColor::Guide).is_empty() && guides_in(&v, ThemeColor::Selected).is_empty());
    e.set_tool(ToolKind::Box);
    hover(&mut e, DVec3::new(-400.0, 503.0, 0.0));
    assert_ne!(e.inference().map(|i| i.1), Some(crate::InferKind::OnGuide));
}

#[test]
fn an_open_part_s_guides_are_its_own_to_select() {
    use skraft_model::GuideShape;
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
    use skraft_model::GuideShape;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    draw_box(&mut e, DVec3::ZERO, DVec3::new(300.0, 200.0, 0.0), "100");
    e.select(vec![]);
    // Seen askew: the top front edge is clicked where its front face is.
    let area = crate::camera::ViewArea::full(e.viewport_size());
    e.camera.set_standard_view(StandardView::Iso, glam::DMat3::IDENTITY, Some((DVec3::splat(-500.0), DVec3::splat(500.0))), area);
    e.set_tool(ToolKind::Measure);
    let on_edge = e.camera.project(DVec3::new(50.0, 0.0, 100.0), e.viewport_size()).unwrap() + DVec2::new(0.0, 1.0);
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

/// The boxes the view draws in the theme's colour `c` (the selection's, the context's).
fn boxes_in(v: &crate::View, c: ThemeColor) -> Vec<crate::OrientedBox> {
    v.drawing.boxes.iter().filter(|b| b.pen.color == crate::draw::Color::Theme(c)).map(|b| b.bounds).collect()
}

/// The guides' strokes the view draws in the theme's colour `c`.
fn guides_in(v: &crate::View, c: ThemeColor) -> Vec<crate::draw::Line> {
    let guide = |l: &&crate::draw::Line| l.pen.color == crate::draw::Color::Theme(c) && l.pen.stipple == crate::draw::Stipple::LongDashes;
    v.drawing.lines.iter().filter(guide).copied().collect()
}

/// The edges' theme colour, the open part's bases' and turned axes'.
fn edge_color() -> crate::draw::Color {
    crate::draw::Color::Theme(crate::draw::ThemeColor::Edge)
}

/// The open part's bodies' bottom caps the view draws: bold, in the edges' colour.
fn bases_of(v: &crate::View) -> Vec<crate::draw::Line> {
    v.drawing.lines.iter().filter(|l| l.pen.color == edge_color() && l.pen.width == crate::draw::BOLD).copied().collect()
}

/// The lines a preview draws seen through faces (dashed where hidden): a measure's.
fn x_rays(p: &crate::tools::Preview) -> Vec<crate::draw::Line> {
    p.drawing.lines.iter().filter(|l| l.pen.hidden.is_some()).copied().collect()
}

#[test]
fn the_tape_draws_a_guide_from_an_axis() {
    use skraft_model::GuideShape;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    e.set_tool(ToolKind::Measure);
    // On the red axis, in the void: a guide along it, offset level.
    click(&mut e, DVec3::new(-300.0, 4.0, 0.0));
    hover(&mut e, DVec3::new(-300.0, 150.0, 0.0));
    // The axis referred to, across the view: in the scene, in its colour.
    let refs: Vec<_> = e
        .preview()
        .drawing
        .lines
        .into_iter()
        .filter(|l| l.pen.layer == crate::draw::Layer::Scene && l.pen.hidden.is_none() && l.pen.color == crate::tools::RED.into())
        .collect();
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
        let (next, out) =
            d.apply(&Operation::AddPrimitive { context: root, primitive, placement: glam::DAffine3::IDENTITY, material: None }).unwrap();
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
    let (mut d, out) = doc
        .apply(&Operation::AddPrimitive { context: root, primitive: board, placement: glam::DAffine3::IDENTITY, material: None })
        .unwrap();
    let a = out.created[0];
    for &x in xs {
        let drill = Primitive::Cylinder { origin: DVec3::new(x, 150.0, -10.0), radius: 5.0, height: 40.0 };
        let (next, out) = d
            .apply(&Operation::AddPrimitive { context: root, primitive: drill, placement: glam::DAffine3::IDENTITY, material: None })
            .unwrap();
        let tool = EntityPath::root(out.created[0]);
        d = next.apply(&Operation::boolean(EntityPath::root(a), tool, Boolean::Difference, skraft_model::Scope::All)).unwrap().0;
    }
    let mut e = top_editor(d);
    e.select(vec![a]);
    e.set_tool(ToolKind::Stretch);
    (e, a)
}

/// The board's blank extent along X, and its holes' places.
fn stretched(e: &Editor, a: Id) -> ((f64, f64), Vec<f64>) {
    let (_, p, _) = e.document().resolve_part(&EntityPath::root(a)).unwrap();
    let (lo, hi) = p.blank().vertex_bounds().unwrap();
    let mut xs: Vec<f64> = p.machinings().iter().map(|m| (m.placement.translation.x * 1e6).round() / 1e6).collect();
    xs.sort_by(f64::total_cmp);
    ((lo.x, hi.x), xs)
}

#[test]
fn stretch_tool_draws_its_box_dotted_but_around_a_box() {
    let dotted = |e: &Editor| -> Vec<(DVec3, DVec3)> {
        let v = e.view();
        v.drawing
            .boxes
            .iter()
            .filter(|b| !b.shell_on_top && b.pen.stipple == crate::draw::Stipple::Dotted && b.pen.color == crate::tools::INK.into())
            .map(|b| (b.bounds.frame.transform_point3(b.bounds.lo), b.bounds.frame.transform_point3(b.bounds.hi)))
            .collect()
    };
    // A drilled board: its box dotted in the scene at the grips (OCL's `_preview_stretch_start`),
    // kept where it was while stretching.
    let (mut e, _) = drilled_board_editor(&[100.0]);
    let full = (DVec3::ZERO, DVec3::new(600.0, 300.0, 18.0));
    let near = |b: &[(DVec3, DVec3)]| matches!(b, [(lo, hi)] if close(*lo, full.0) && close(*hi, full.1));
    assert!(near(&dotted(&e)), "{:?}", dotted(&e));
    click(&mut e, DVec3::new(600.0, 150.0, 18.0));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(800.0, 150.0, 18.0)) });
    assert!(near(&dotted(&e)), "stretching: {:?}", dotted(&e));

    // A part that is its box: none at the grips; while stretching, where it was.
    let mut e = top_editor(Document::new());
    let board = draw_box(&mut e, DVec3::ZERO, DVec3::new(600.0, 300.0, 0.0), "18");
    e.select(vec![board]);
    e.set_tool(ToolKind::Stretch);
    assert!(dotted(&e).is_empty(), "{:?}", dotted(&e));
    click(&mut e, DVec3::new(600.0, 150.0, 18.0));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(800.0, 150.0, 18.0)) });
    assert!(near(&dotted(&e)), "stretching: {:?}", dotted(&e));
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
        let mut xs: Vec<i64> = e.view().drawing.fills.iter().map(|f| f.corners[0].x.round() as i64).collect();
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
    let behavior = skraft_model::Behavior { no_stretch: skraft_model::StretchMask(1), cutters: None };
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
        let mut xs: Vec<i64> = e.view().drawing.fills.iter().map(|f| f.corners[0].x.round() as i64).collect();
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
    let bold = |e: &Editor| {
        let over_bold = |l: &&crate::draw::Line| l.pen.layer == crate::draw::Layer::Over && l.pen.width == crate::draw::BOLD;
        e.view().drawing.lines.iter().filter(over_bold).count()
    };
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

    // Taken, the cursor towards the start, 30 typed: moved 30 that way (the typed offset takes
    // the cursor's way, once).
    e.input(ToolInput::Hover { px: px(&e, on_axis(370.0)) });
    click(&mut e, on_axis(370.0));
    assert!(e.status().contains("valider le déplacement"), "{}", e.status());
    e.input(ToolInput::Hover { px: px(&e, on_axis(220.0)) });
    e.input(ToolInput::Vcb("30".into()));
    assert_eq!(stretched(&e, a), ((0.0, 600.0), vec![100.0, 290.0, 500.0]), "{}", e.status());
}

#[test]
fn stretch_tool_refuses_what_cannot_stretch_and_takes_the_open_part() {
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let rod = Primitive::Cylinder { origin: DVec3::ZERO, radius: 50.0, height: 200.0 };
    let rod = e
        .apply(Operation::AddPrimitive { context: root, primitive: rod, placement: glam::DAffine3::IDENTITY, material: None })
        .unwrap()
        .unwrap()
        .created[0];
    e.select(vec![rod]);
    e.set_tool(ToolKind::Stretch);
    // The red axis locked (→): across it, the cylinder cannot stretch.
    e.input(ToolInput::Key { key: Key::Right, mods: Modifiers::default() });
    click(&mut e, DVec3::new(50.0, 0.0, 200.0));
    assert!(e.status().contains("traverse une coupe"), "{}", e.status());
    let before = e.document().resolve_part(&EntityPath::root(rod)).unwrap().1.blank().clone();

    // Along its axis it stretches — the top grip, nearer the eye than the bottom one under it;
    // in the open part, the part's own scope (no option of the tool's).
    e.set_tool(ToolKind::Select);
    e.enter(rod);
    e.set_tool(ToolKind::Stretch);
    assert!(e.tool_options().iter().all(|o| o.key != "scope"));
    click(&mut e, DVec3::new(0.0, 0.0, 200.0));
    e.input(ToolInput::Vcb("300".into()));
    let after = e.document().resolve_part(&EntityPath::root(rod)).unwrap().1.blank().clone();
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
    assert!(e.view().drawing.fills.is_empty());
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
    let turned = |m: &skraft_model::kernel::Mesh| {
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
    assert!(e.view().drawing.fills.is_empty() && e.overlay_points().is_empty());
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
        let (d, out) =
            d.apply(&Operation::AddPrimitive { context: root, primitive: p, placement: glam::DAffine3::IDENTITY, material: None }).unwrap();
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
    let b1 = e
        .apply(Operation::AddPrimitive { context: root, primitive: board, placement: glam::DAffine3::IDENTITY, material: None })
        .unwrap()
        .unwrap()
        .created[0];
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
fn stretch_tool_stretches_a_prism_with_fillets_along_its_height() {
    use skraft_kernel::profile;
    // A triangle with rounded corners, 50 high, seen from above: its top grip, overall 80.
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let triangle = profile::polygon(&[DVec2::ZERO, DVec2::new(400.0, 0.0), DVec2::new(200.0, 300.0)]);
    let prism = Primitive::Prism { origin: DVec3::ZERO, profile: profile::with_fillets(&triangle, 20.0), height: 50.0 };
    let p = e
        .apply(Operation::AddPrimitive { context: root, primitive: prism, placement: glam::DAffine3::IDENTITY, material: None })
        .unwrap()
        .unwrap()
        .created[0];
    e.set_tool(ToolKind::Stretch);
    click(&mut e, DVec3::new(200.0, 100.0, 50.0));
    click(&mut e, DVec3::new(200.0, 150.0, 50.0));
    e.input(ToolInput::Vcb("80".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(p));
    assert!((lo.z, hi.z) == (0.0, 80.0), "{lo} {hi} {}", e.status());
    // Its fillets kept as radii: the profile follows.
    let (_, part, _) = e.document().resolve_part(&EntityPath::root(p)).unwrap();
    let blank = part.profiled().and_then(|b| part.profile_of(b)).expect("a profile");
    assert_eq!(blank.extrusion().map(|e| e.1), Some(80.0));
    assert!(blank.profile.iter().all(|c| c.fillet == 20.0), "{:?}", blank.profile);
}

#[test]
fn stretch_tool_stretches_along_the_axes_chosen() {
    // A board turned square: its own X along the world's Y (−50–0 along X, 0–200 along Y).
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let board = Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(200.0, 50.0, 18.0) };
    let turn = glam::DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_2);
    let b =
        e.apply(Operation::AddPrimitive { context: root, primitive: board, placement: turn, material: None }).unwrap().unwrap().created[0];
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
    e.apply(Operation::AddPrimitive { context: root, primitive: board, placement: turn, material: None }).unwrap().unwrap();
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
                    material: None,
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
        glam::DMat3::IDENTITY,
        Some((DVec3::ZERO, DVec3::new(900.0, 900.0, 200.0))),
        crate::camera::ViewArea::full(e.viewport_size()),
    );
    let picks_as_a_scan = |e: &Editor| {
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
    };
    picks_as_a_scan(&e);
    // Some moved a little, then others far: the index refit, then built again once loose.
    for (k, by) in [DVec3::new(5.0, -3.0, 2.0), DVec3::new(-300.0, 450.0, 60.0)].into_iter().enumerate() {
        let some: Vec<Id> = made[20..].iter().copied().skip(k).step_by(3).collect();
        e.apply(Operation::Transform { context: root, entities: some, transform: glam::DAffine3::from_translation(by) }).unwrap();
        picks_as_a_scan(&e);
    }
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
                material: None,
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
    e.import("assembly.step".into(), include_bytes!("../../kernel/tests/fixtures/assembly.step").to_vec());
    assert!(!e.is_importing());
    let doc = e.document();
    assert_eq!(doc.root_assembly().occurrences().len(), 1);
    assert_eq!(doc.placed_parts().len(), 2);
    // What came in is selected; one undo takes it all out.
    assert_eq!(e.selection(), &[doc.root_assembly().occurrences()[0].id][..]);
    let said = notices(&mut e);
    assert!(said.contains("assembly.step importé"), "{said}");
    e.undo();
    assert!(e.document().root_assembly().occurrences().is_empty());
}

/// A SketchUp file goes the same way as a STEP one, told by its extension
/// (docs/io/sketchup-import.md): read by the kernel, inserted in one step, a toast saying the
/// parts and the definitions excluded.
#[test]
fn a_sketchup_file_is_imported_into_the_context_in_one_step() {
    let mut e = top_editor(Document::new());
    e.import("cabinet.skp".into(), include_bytes!("../../kernel/tests/fixtures/cabinet.skp").to_vec());
    assert!(!e.is_importing());
    let doc = e.document();
    // The model's top (two cabinets, the long board, the plinth, the cube) in one assembly
    // named after the file.
    assert_eq!(doc.root_assembly().occurrences().len(), 1);
    let top = doc.component(doc.root_assembly().occurrences()[0].component).unwrap();
    assert_eq!(top.name, "cabinet.skp");
    assert_eq!(doc.placed_parts().len(), 8);
    assert_eq!(e.selection(), &[doc.root_assembly().occurrences()[0].id][..]);
    // The success, then what was left out as a warning of its own.
    let said: Vec<_> = e.take_notices().into_iter().map(|n| (n.kind, n.title)).collect();
    assert_eq!(
        said,
        [(NoticeKind::Success, "cabinet.skp importé : 5 pièces".to_owned()), (NoticeKind::Warning, "1 corps non solide ignoré".to_owned())]
    );
    e.undo();
    assert!(e.document().root_assembly().occurrences().is_empty());
}

#[test]
fn the_selection_drawn_follows_what_it_hangs_on() {
    // Kept from frame to frame, the selection's marks and boxes are worked out again whenever
    // the selection, the document, the context or the ghost changes.
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "50");
    let b = draw_box(&mut e, DVec3::new(300.0, 0.0, 0.0), DVec3::new(400.0, 100.0, 0.0), "50");
    let root = e.document().root();
    let selected =
        |e: &Editor| e.view().highlights.iter().filter(|h| h.mark == Mark::Selected).map(|h| h.transform.translation).collect::<Vec<_>>();
    let box_lo = |e: &Editor| boxes_in(&e.view(), ThemeColor::Selected).iter().map(|b| b.frame.transform_point3(b.lo)).collect::<Vec<_>>();

    e.select(vec![a]);
    assert_eq!(selected(&e).len(), 1);
    assert!(box_lo(&e).iter().all(|p| p.abs_diff_eq(DVec3::ZERO, 1e-3)), "{:?}", box_lo(&e));
    // The selection changes: the other box.
    e.select(vec![b]);
    assert!(box_lo(&e).iter().all(|p| p.abs_diff_eq(DVec3::new(300.0, 0.0, 0.0), 1e-3)), "{:?}", box_lo(&e));
    // The document changes (the selection the same): drawn where it went, and back on undo.
    e.apply(Operation::Transform { context: root, entities: vec![b], transform: glam::DAffine3::from_translation(DVec3::Y * 500.0) })
        .unwrap();
    assert!(box_lo(&e).iter().all(|p| p.abs_diff_eq(DVec3::new(300.0, 500.0, 0.0), 1e-3)), "{:?}", box_lo(&e));
    e.undo();
    e.select(vec![b]);
    assert!(box_lo(&e).iter().all(|p| p.abs_diff_eq(DVec3::new(300.0, 0.0, 0.0), 1e-3)), "{:?}", box_lo(&e));
    // Both: two marks, two boxes.
    e.select(vec![a, b]);
    assert_eq!((selected(&e).len(), box_lo(&e).len()), (2, 2));
    // A move's ghost: the marks and boxes moved with it, the original hidden.
    e.set_tool(ToolKind::Move);
    click(&mut e, DVec3::new(0.0, 0.0, 50.0));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(0.0, 200.0, 50.0)) });
    let v = e.view();
    let ghosts: Vec<DVec3> = v.highlights.iter().filter(|h| h.mark == Mark::Ghost).map(|h| h.transform.translation).collect();
    assert_eq!(ghosts.len(), 2);
    assert!(ghosts.iter().any(|t| (t.y - 200.0).abs() < 1e-3), "{ghosts:?}");
    assert!(boxes_in(&v, ThemeColor::Selected).iter().all(|b| (b.frame.transform_point3(b.lo).y - 200.0).abs() < 1e-3));
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    // Inside an assembly: its occurrences are the entities, marked where they are.
    e.set_tool(ToolKind::Select);
    let asm = e.apply(Operation::MakeAssembly { context: root, entities: vec![a, b], name: None }).unwrap().unwrap().created[0];
    e.enter(asm);
    e.select_all();
    assert_eq!((selected(&e).len(), box_lo(&e).len()), (2, 2));
    e.select(vec![]);
    assert_eq!((selected(&e).len(), box_lo(&e).len()), (0, 0));
}

#[test]
fn meshes_taken_in_leave_the_scene_a_sync_would() {
    // Distinct parts and copies, one in an assembly: drawn as stand-ins until their meshes
    // come, then taken in without a sync — as a sync would have left the scene.
    let mut doc = Document::new();
    let root = doc.root();
    let mut made = Vec::new();
    for k in 0..6 {
        let primitive = if k % 3 == 2 {
            Primitive::Cylinder { origin: DVec3::new(k as f64 * 200.0, 0.0, 0.0), radius: 40.0, height: 60.0 }
        } else {
            Primitive::Box { origin: DVec3::new(k as f64 * 200.0, 0.0, 0.0), size: DVec3::new(100.0 + k as f64 * 10.0, 80.0, 30.0) }
        };
        let (d, out) =
            doc.apply(&Operation::AddPrimitive { context: root, primitive, placement: glam::DAffine3::IDENTITY, material: None }).unwrap();
        doc = d;
        made.push(out.created[0]);
    }
    let copy = glam::DAffine3::from_translation(DVec3::Y * 300.0);
    doc = doc.apply(&Operation::Duplicate { context: root, entities: made[..3].to_vec(), transform: copy }).unwrap().0;
    doc = doc.apply(&Operation::MakeAssembly { context: root, entities: vec![made[4], made[5]], name: None }).unwrap().0;
    let (mut e, k) = deferred_editor(doc);
    let state = |e: &Editor| {
        let s = e.scene();
        let meshes: Vec<_> = s.definition_meshes().map(|d| (d.definition, d.revision, d.parts.first().and_then(|p| p.bounds))).collect();
        // Rays through a grid over the parts (seen from above).
        let picks: Vec<Option<(EntityPath, u32)>> = (0..54)
            .flat_map(|x| (0..17).map(move |y| DVec3::new(x as f64 * 25.0 - 50.0, y as f64 * 25.0 - 20.0, 0.0)))
            .map(|p| s.pick(&e.camera.ray(px(e, p), e.viewport_size()), |_| true).map(|h| (h.path, h.face)))
            .collect();
        (meshes, s.bounds(), picks)
    };
    // Stand-ins: outlines, nothing to pick.
    assert!(k.queued() > 0);
    assert!(state(&e).2.iter().all(Option::is_none));
    k.run_all();
    let generation = e.scene().generation();
    e.poll();
    assert!(e.scene().generation() > generation);
    let taken_in = state(&e);
    assert!(taken_in.2.iter().filter(|p| p.is_some()).count() > 20, "{:?}", taken_in.2);
    e.sync_scene();
    assert_eq!(taken_in, state(&e));
}

#[test]
fn erase_tool_erases_what_it_is_clicked_or_dragged_over() {
    use skraft_model::GuideShape;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "50");
    let b = draw_box(&mut e, DVec3::new(300.0, 0.0, 0.0), DVec3::new(400.0, 100.0, 0.0), "50");
    let c = draw_box(&mut e, DVec3::new(600.0, 0.0, 0.0), DVec3::new(700.0, 100.0, 0.0), "50");
    let line = GuideShape::Line { origin: DVec3::new(0.0, -300.0, 0.0), dir: DVec3::X };
    let g = e.apply(Operation::AddGuide { component: root, shape: line }).unwrap().unwrap().guide.unwrap();
    let none = Modifiers::default();
    let shift = Modifiers { toggle: true, copy: false };
    let there = |e: &Editor, id: Id| e.document().assembly(root).unwrap().occurrence(id).is_some();
    let ghosts = |e: &Editor| e.view().batches.iter().flat_map(|(_, p)| p).filter(|p| p.ghost).count();
    // Started on a selection, it lets it go, erasing nothing (docs/tools/selection.md).
    e.select(vec![a]);
    e.set_tool(ToolKind::Erase);
    assert!(there(&e, a) && e.selection().is_empty());
    assert_eq!(e.tool_kind(), ToolKind::Erase);
    // Selected while it runs, the selection is left alone.
    e.select(vec![a]);

    // Hovered: drawn as a ghost.
    hover(&mut e, DVec3::new(350.0, 50.0, 50.0));
    assert_eq!(ghosts(&e), 1);

    // Dragged from `a` to `b` in one move: both taken (the gap is swept), drawn as ghosts, the
    // selected one unmarked; erased at once when released, in one step.
    e.input(ToolInput::DragStart { px: px(&e, DVec3::new(50.0, 50.0, 50.0)), mods: none });
    e.input(ToolInput::Drag { px: px(&e, DVec3::new(350.0, 50.0, 50.0)) });
    assert_eq!(ghosts(&e), 2);
    assert!(!e.view().highlights.iter().any(|h| h.mark == Mark::Selected), "a ghost is not marked");
    assert!(there(&e, a) && there(&e, b), "nothing erased before the release");
    e.input(ToolInput::DragEnd { px: px(&e, DVec3::new(350.0, 50.0, 50.0)), mods: none });
    assert!(!there(&e, a) && !there(&e, b) && there(&e, c));
    assert!(e.selection().is_empty());
    assert_eq!(ghosts(&e), 0);
    e.undo();
    assert!(there(&e, a) && there(&e, b), "one undo brings both back");

    // Esc drops the drag.
    e.input(ToolInput::DragStart { px: px(&e, DVec3::new(650.0, 50.0, 50.0)), mods: none });
    e.input(ToolInput::Drag { px: px(&e, DVec3::new(660.0, 50.0, 50.0)) });
    e.input(ToolInput::Key { key: Key::Escape, mods: none });
    e.input(ToolInput::DragEnd { px: px(&e, DVec3::new(660.0, 50.0, 50.0)), mods: none });
    assert!(there(&e, c));
    assert_eq!(e.tool_kind(), ToolKind::Erase);

    // A click near the guide erases it.
    click(&mut e, DVec3::new(100.0, -303.0, 0.0));
    assert!(e.document().component(root).unwrap().guides.iter().all(|x| x.id != g));
    e.undo();

    // With Shift, the occurrences swept are hidden, the guide is left.
    drag(&mut e, DVec3::new(650.0, 50.0, 50.0), DVec3::new(650.0, -320.0, 0.0), shift);
    let assembly = e.document().assembly(root).unwrap();
    assert!(assembly.occurrence(c).unwrap().hidden);
    assert!(!assembly.occurrence(a).unwrap().hidden);
    assert_eq!(e.document().component(root).unwrap().guides.len(), 1);
}

#[test]
fn erase_tool_erases_the_open_part_s_machinings() {
    use glam::DAffine3;
    use skraft_model::Scope;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let board = draw_box(&mut e, DVec3::ZERO, DVec3::new(400.0, 200.0, 0.0), "18");
    let drill = e
        .apply(Operation::AddPrimitive {
            context: root,
            primitive: Primitive::Cylinder { origin: DVec3::new(50.0, 100.0, -5.0), radius: 4.0, height: 30.0 },
            placement: DAffine3::IDENTITY,
            material: None,
        })
        .unwrap()
        .unwrap()
        .created[0];
    e.apply(Operation::boolean(EntityPath::root(board), EntityPath::root(drill), Boolean::Difference, Scope::All)).unwrap();
    let machinings = |e: &Editor| e.document().resolve_part(&EntityPath::root(board)).unwrap().1.machinings().len();
    assert_eq!(machinings(&e), 1);
    e.enter(board);
    e.set_tool(ToolKind::Erase);
    hover(&mut e, DVec3::new(50.0, 100.0, 25.0));
    assert!(e.view().batches.iter().flat_map(|(_, p)| p).any(|p| p.ghost && p.look == Look::Tool), "the tool drawn as a ghost");
    click(&mut e, DVec3::new(50.0, 100.0, 25.0));
    assert_eq!(machinings(&e), 0, "{}", e.status());
    assert_eq!(e.open_part(), Some(board));
}

// ---- Turn tool (docs/tools/turned-bodies.md) --------------------------------------------------

/// The world bounds (exact) of the part an occurrence path names.
fn exact_bounds_of(e: &Editor, path: &EntityPath) -> (DVec3, DVec3) {
    let (_, part, t) = e.document().resolve_part(path).unwrap();
    crate::scene::transform_bounds(&t, part.shape().bounds().unwrap())
}

#[test]
fn turn_by_an_axis_and_a_half_profile() {
    let mut e = top_editor(Document::new());
    e.set_tool(ToolKind::Turn);
    assert!(e.status().contains("axe"), "{}", e.status());
    // The axis along green, 600 long (typed); seen from above, the profile is drawn on the ground.
    click(&mut e, DVec3::ZERO);
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(0.0, 300.0, 0.0)) });
    e.input(ToolInput::Vcb("600".into()));
    assert!(e.status().contains("demi-profil"), "{}", e.status());
    // A rectangle beside the axis, closed on its first corner: turned at once.
    for (x, y) in [(0.0, 0.0), (100.0, 0.0), (100.0, 600.0), (0.0, 600.0)] {
        click(&mut e, DVec3::new(x, y, 0.0));
    }
    // While drawing, the mirror across the axis shows, dashed — closed: with the cursor off
    // the corners, its side back to the first corner too.
    assert!(e.preview().drawing.lines.iter().any(|l| l.a.x < -50.0), "the mirror");
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(50.0, 700.0, 0.0)) });
    let closing = |l: &crate::draw::Line| {
        let m = (l.a + l.b) / 2.0;
        m.x < -1.0 && m.x > -49.0 && m.y > 10.0 && m.y < 600.0
    };
    assert!(e.preview().drawing.lines.iter().any(closing), "the mirror closed");
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(0.0, 600.0, 0.0)) });
    click(&mut e, DVec3::ZERO);
    let id = drawn(&e);
    let (lo, hi) = exact_bounds_of(&e, &EntityPath::root(id));
    assert!(close(lo, DVec3::new(-100.0, 0.0, -100.0)) && close(hi, DVec3::new(100.0, 600.0, 100.0)), "{lo} {hi}");
    // A turned body, its axes' Z along the axis.
    let (_, part, t) = e.document().resolve_part(&EntityPath::root(id)).unwrap();
    let b = part.profiled().unwrap();
    assert!(part.profile_of(b).unwrap().is_turned());
    assert!(close((t * b.placement).matrix3.z_axis, DVec3::Y));
    // Back to the axis's first point.
    assert!(e.status().contains("premier point de l'axe"), "{}", e.status());

    // On the other side of the axis, and across it.
    click(&mut e, DVec3::new(500.0, 0.0, 0.0));
    click(&mut e, DVec3::new(500.0, 600.0, 0.0));
    for (x, y) in [(500.0, 0.0), (350.0, 0.0), (350.0, 600.0), (500.0, 600.0), (500.0, 0.0)] {
        click(&mut e, DVec3::new(x, y, 0.0));
    }
    let id = drawn(&e);
    let (lo, hi) = exact_bounds_of(&e, &EntityPath::root(id));
    assert!(close(lo, DVec3::new(350.0, 0.0, -150.0)) && close(hi, DVec3::new(650.0, 600.0, 150.0)), "{lo} {hi}");
    let before = e.document().assembly(e.document().root()).unwrap().occurrences().len();
    click(&mut e, DVec3::new(800.0, 0.0, 0.0));
    click(&mut e, DVec3::new(800.0, 600.0, 0.0));
    for (x, y) in [(700.0, -200.0), (900.0, -200.0), (900.0, 400.0), (700.0, -200.0)] {
        click(&mut e, DVec3::new(x, y, 0.0));
    }
    assert_eq!(e.document().assembly(e.document().root()).unwrap().occurrences().len(), before, "refused");
    assert!(e.status().contains("traverse"), "{}", e.status());
    // Esc takes the corners back, then the axis, end first.
    for _ in 0..3 {
        key(&mut e, Key::Escape);
    }
    assert!(e.status().contains("demi-profil"), "{}", e.status());
    key(&mut e, Key::Escape);
    assert!(e.status().contains("second point"), "{}", e.status());
    key(&mut e, Key::Escape);
    assert!(e.status().contains("premier point de l'axe"), "{}", e.status());
}

#[test]
fn turn_s_plane_goes_through_the_axis_and_the_first_point() {
    let mut e = top_editor(Document::new());
    // A block standing 200 high beside the axis: the first point taken on its top.
    draw_box(&mut e, DVec3::new(50.0, 250.0, 0.0), DVec3::new(250.0, 450.0, 0.0), "200");
    e.select(Vec::new());
    e.set_tool(ToolKind::Turn);
    click(&mut e, DVec3::ZERO);
    click(&mut e, DVec3::new(0.0, 600.0, 0.0));
    let first = DVec3::new(120.0, 330.0, 200.0);
    click(&mut e, first);
    // Then two points on the axis, and back to the first: a cone, its plane tilted.
    click(&mut e, DVec3::new(0.0, 330.0, 0.0));
    click(&mut e, DVec3::new(0.0, 560.0, 0.0));
    click(&mut e, first);
    let id = drawn(&e);
    let (_, part, t) = e.document().resolve_part(&EntityPath::root(id)).unwrap();
    let b = part.profiled().unwrap();
    assert!(part.profile_of(b).unwrap().is_turned());
    // The body's X towards the first point, square to the axis.
    let x = (t * b.placement).matrix3.x_axis;
    assert!(close(x, DVec3::new(120.0, 0.0, 200.0).normalize()), "{x}");
}

#[test]
fn turn_s_plane_waits_for_a_point_off_the_axis() {
    let mut e = top_editor(Document::new());
    draw_box(&mut e, DVec3::new(50.0, 250.0, 0.0), DVec3::new(250.0, 450.0, 0.0), "200");
    e.select(Vec::new());
    e.set_tool(ToolKind::Turn);
    click(&mut e, DVec3::ZERO);
    click(&mut e, DVec3::new(0.0, 600.0, 0.0));
    // The first point on the axis: no plane yet.
    click(&mut e, DVec3::new(0.0, 100.0, 0.0));
    // A point over nothing, then taken back: the plane free again.
    click(&mut e, DVec3::new(-150.0, 100.0, 0.0));
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    // A second on the axis still leaves it free; the next, on the block's top, sets it.
    click(&mut e, DVec3::new(0.0, 560.0, 0.0));
    let off = DVec3::new(120.0, 330.0, 200.0);
    click(&mut e, off);
    click(&mut e, DVec3::new(0.0, 100.0, 0.0));
    let id = drawn(&e);
    let (_, part, t) = e.document().resolve_part(&EntityPath::root(id)).unwrap();
    let b = part.profiled().unwrap();
    assert!(part.profile_of(b).unwrap().is_turned());
    let x = (t * b.placement).matrix3.x_axis;
    assert!(close(x, DVec3::new(120.0, 0.0, 200.0).normalize()), "{x}");
}

#[test]
fn turn_s_profile_infers_on_the_axis_and_its_origin() {
    use crate::infer::{InferKind, LineKind};
    let mut e = top_editor(Document::new());
    e.set_tool(ToolKind::Turn);
    // A slanted axis (along no axis of the context's): only its origin marked, no length.
    let (a, b) = (DVec3::new(100.0, 0.0, 0.0), DVec3::new(400.0, 300.0, 0.0));
    click(&mut e, a);
    e.input(ToolInput::Hover { px: px(&e, b) });
    let p = e.preview();
    assert!(p.drawing.lengths.is_empty(), "no length: {:?}", p.drawing.lengths.len());
    assert_eq!(p.drawing.grips.len(), 1, "the origin only");
    click(&mut e, b);
    assert_eq!(e.preview().drawing.grips.len(), 1, "the origin only");
    // The profile's first point snaps to the axis's origin.
    e.input(ToolInput::Hover { px: px(&e, a + DVec3::new(6.0, -4.0, 0.0)) });
    let inf = e.preview().inference.expect("an inference");
    assert_eq!(inf.kind, InferKind::Endpoint);
    assert!(close(inf.point, a), "{}", inf.point);
    // Then a corner off the axis, and the next one onto the axis.
    click(&mut e, DVec3::new(350.0, 100.0, 0.0));
    let on = DVec3::new(250.0, 150.0, 0.0);
    e.input(ToolInput::Hover { px: px(&e, on + DVec3::new(4.0, -4.0, 0.0)) });
    let inf = e.preview().inference.expect("an inference");
    assert_eq!(inf.kind, InferKind::OnLine(LineKind::Tool), "{:?}", inf.kind);
    let off = inf.point - a;
    assert!(off.cross((b - a).normalize()).length() < 1e-6, "on the axis: {}", inf.point);
}

/// A turned leg drawn from above: its axis along green from the origin, 600 long, its half
/// profile 100 wide on the side of red; the part open in the Profile tool.
fn turned_leg_editor() -> (Editor, Id) {
    let mut e = top_editor(Document::new());
    e.set_tool(ToolKind::Turn);
    click(&mut e, DVec3::ZERO);
    click(&mut e, DVec3::new(0.0, 600.0, 0.0));
    for (x, y) in [(0.0, 0.0), (100.0, 0.0), (100.0, 600.0), (0.0, 600.0), (0.0, 0.0)] {
        click(&mut e, DVec3::new(x, y, 0.0));
    }
    let id = drawn(&e);
    e.enter(id);
    e.set_tool(ToolKind::Profile);
    (e, id)
}

#[test]
fn a_turned_profile_is_edited_in_its_plane_facing_the_eye() {
    use crate::tools::PointShape;
    let (mut e, id) = turned_leg_editor();
    let path = EntityPath::root(id);
    // The half profile and its axis drawn bold and dashed while the part is open.
    let v = e.view();
    let axes: Vec<_> =
        v.drawing.lines.iter().filter(|l| l.pen.color == edge_color() && l.pen.stipple != crate::draw::Stipple::Solid).collect();
    assert!(!bases_of(&v).is_empty() && axes.len() == 1, "{axes:?}");
    // Seen from above, the half profile lies on the ground: its grips there, none across the
    // side on the axis.
    let shape_at = |e: &Editor, p: DVec3| e.overlay_points().into_iter().find(|g| (g.at - p).length() < 1e-3).map(|g| g.shape);
    assert_eq!(shape_at(&e, DVec3::new(100.0, 600.0, 0.0)), Some(PointShape::Square));
    assert!(matches!(shape_at(&e, DVec3::new(100.0, 300.0, 0.0)), Some(PointShape::Bar { .. })));
    assert_eq!(shape_at(&e, DVec3::new(0.0, 300.0, 0.0)), None);
    // The outer side moved out: Ø 300.
    click(&mut e, DVec3::new(100.0, 300.0, 0.0));
    click(&mut e, DVec3::new(150.0, 300.0, 0.0));
    let (lo, hi) = exact_bounds_of(&e, &path);
    assert!(close(lo, DVec3::new(-150.0, 0.0, -150.0)) && close(hi, DVec3::new(150.0, 600.0, 150.0)), "{lo} {hi}");
    // A corner on the axis slides along it: the top up to 700.
    click(&mut e, DVec3::new(0.0, 600.0, 0.0));
    click(&mut e, DVec3::new(60.0, 700.0, 0.0));
    let (lo, hi) = exact_bounds_of(&e, &path);
    assert!(close(lo, DVec3::new(-150.0, 0.0, -150.0)) && close(hi, DVec3::new(150.0, 700.0, 150.0)), "{lo} {hi}");
    // Free, a corner pulled across the axis stops on it.
    key(&mut e, Key::Copy);
    click(&mut e, DVec3::new(150.0, 700.0, 0.0));
    click(&mut e, DVec3::new(-200.0, 700.0, 0.0));
    let p = e.document().resolve_part(&path).unwrap().1;
    let b = p.profile_of(p.profiled().unwrap()).unwrap();
    assert!(b.is_turned() && b.profile.iter().all(|c| c.p.x >= -1e-9), "{:?}", b.profile);
    assert!(b.profile.iter().any(|c| c.p.x.abs() < 1e-3 && (c.p.y - 700.0).abs() < 1e-3), "{:?}", b.profile);
}

#[test]
fn a_turned_part_stretches_only_along_its_axis() {
    let mut e = top_editor(Document::new());
    e.set_tool(ToolKind::Turn);
    click(&mut e, DVec3::ZERO);
    click(&mut e, DVec3::new(0.0, 600.0, 0.0));
    for (x, y) in [(0.0, 0.0), (100.0, 0.0), (100.0, 600.0), (0.0, 600.0), (0.0, 0.0)] {
        click(&mut e, DVec3::new(x, y, 0.0));
    }
    let id = drawn(&e);
    e.select(vec![id]);
    e.set_tool(ToolKind::Stretch);
    // Its axes across the turning axis locked for good: two padlocks, the axis's grips.
    let padlocks = |e: &Editor| {
        e.overlay_points().into_iter().filter(|p| p.shape == crate::tools::PointShape::Padlock).map(|p| p.at).collect::<Vec<_>>()
    };
    let locks = padlocks(&e);
    assert_eq!(locks.len(), 2, "{locks:?}");
    e.input(ToolInput::Hover { px: px(&e, locks[0]) });
    assert!(e.status().contains("tourné"), "{}", e.status());
    click(&mut e, locks[0]);
    assert_eq!(padlocks(&e).len(), 2);
    let part = e.document().resolve(&EntityPath::root(id)).unwrap().component;
    assert!(e.document().component(part).unwrap().behavior.is_default(), "the lock is not saved");
}

// ---- Prism tool (docs/tools/prism.md) ---------------------------------------------------

fn key(e: &mut Editor, key: Key) {
    e.input(ToolInput::Key { key, mods: Modifiers::default() });
}

/// The corners clicked one after the other with the prism tool (the first one included).
fn corners(e: &mut Editor, ps: &[(f64, f64)]) {
    for &(x, y) in ps {
        click(e, DVec3::new(x, y, 0.0));
    }
}

#[test]
fn prism_by_clicks_closed_on_the_first_corner() {
    let mut e = top_editor(Document::new());
    e.set_tool(ToolKind::Prism);
    // An L, closed by a click back on its first corner, then a typed height.
    corners(&mut e, &[(0.0, 0.0), (400.0, 0.0), (400.0, 100.0), (100.0, 100.0), (100.0, 300.0), (0.0, 300.0), (0.0, 0.0)]);
    assert!(e.status().contains("hauteur"), "{}", e.status());
    e.input(ToolInput::Vcb("18".into()));
    let id = drawn(&e);
    let (lo, hi) = bounds_of(&e, &EntityPath::root(id));
    assert!(close(lo, DVec3::ZERO) && close(hi, DVec3::new(400.0, 300.0, 18.0)), "{lo} {hi}");
    // An L prism: 8 faces, 18 edges, 12 vertices.
    let (_, part, _) = e.document().resolve_part(&EntityPath::root(id)).unwrap();
    assert_eq!(part.shape().topology_counts(), (8, 18, 12));
}

#[test]
fn prism_follows_the_cursor_right_after_a_click() {
    let mut e = top_editor(Document::new());
    e.set_tool(ToolKind::Prism);
    corners(&mut e, &[(0.0, 0.0), (300.0, 0.0)]);
    // No move since the click: the line from the new corner and the closing one are drawn.
    let lines = e.preview().drawing.lines;
    let at = |p: DVec3| lines.iter().any(|l| close(l.a, p) || close(l.b, p));
    assert!(at(DVec3::ZERO) && at(DVec3::new(300.0, 0.0, 0.0)), "{lines:?}");
    assert!(lines.len() > 2, "the side, the line to the cursor and the closing one: {}", lines.len());

    // From three corners, the cursor is on the corner just placed: the closing side is solid
    // from it to the first one, as the other sides.
    click(&mut e, DVec3::new(300.0, 200.0, 0.0));
    let lines = e.preview().drawing.lines;
    let (a, b) = (DVec3::new(300.0, 200.0, 0.0), DVec3::ZERO);
    let on =
        |p: DVec3| (p - a).cross(b - a).length() < 1e-3 * (b - a).length() && (p - a).dot(b - a) >= -1e-6 && (p - b).dot(a - b) >= -1e-6;
    let pieces = lines.iter().filter(|l| on(l.a) && on(l.b) && l.a.distance(l.b) > 1.0).count();
    assert!(lines.iter().filter(|l| on(l.a) && on(l.b)).all(|l| l.pen.stipple == crate::draw::Stipple::Solid));
    assert_eq!(pieces, 1, "the closing side solid, in one piece");
}

#[test]
fn prism_esc_enter_reversed_and_typed() {
    let mut e = top_editor(Document::new());
    e.set_tool(ToolKind::Prism);
    // A wrong corner taken away with Esc.
    corners(&mut e, &[(0.0, 0.0), (200.0, 0.0), (500.0, 500.0)]);
    key(&mut e, Key::Escape);
    // A length typed along the cursor's direction: up 100.
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(200.0, 50.0, 0.0)) });
    e.input(ToolInput::Vcb("100".into()));
    // Reversed: the next corner goes before the first one, the lines start from it.
    key(&mut e, Key::Copy);
    click(&mut e, DVec3::new(0.0, 100.0, 0.0));
    key(&mut e, Key::Copy);
    // Enter closes: a rectangle 200 × 100 (the corner (0,100) went before (0,0)).
    key(&mut e, Key::Enter);
    e.input(ToolInput::Vcb("10".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(close(lo, DVec3::ZERO) && close(hi, DVec3::new(200.0, 100.0, 10.0)), "{lo} {hi}");
}

#[test]
fn prism_refuses_a_crossed_outline_and_too_few_corners() {
    let mut e = top_editor(Document::new());
    e.set_tool(ToolKind::Prism);
    corners(&mut e, &[(0.0, 0.0), (200.0, 0.0)]);
    key(&mut e, Key::Enter);
    assert!(e.status().contains("trois points"), "{}", e.status());
    // A bow tie.
    corners(&mut e, &[(0.0, 200.0), (200.0, 200.0)]);
    key(&mut e, Key::Enter);
    assert!(e.status().contains("se croise"), "{}", e.status());
    // Esc takes the last corner away: a triangle closes.
    key(&mut e, Key::Escape);
    key(&mut e, Key::Enter);
    e.input(ToolInput::Vcb("5".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(close(lo, DVec3::ZERO) && close(hi, DVec3::new(200.0, 200.0, 5.0)), "{lo} {hi}");
}

#[test]
fn prism_double_click_closes_without_pulling() {
    let mut e = top_editor(Document::new());
    // A height to take again, so a double-click that pulled would be seen.
    draw_box(&mut e, DVec3::new(-900.0, -900.0, 0.0), DVec3::new(-800.0, -800.0, 0.0), "77");
    let count = |e: &Editor| e.document().assembly(e.document().root()).unwrap().occurrences().len();
    let before = count(&e);
    e.set_tool(ToolKind::Prism);
    corners(&mut e, &[(0.0, 0.0), (300.0, 0.0)]);
    let at = px(&e, DVec3::new(300.0, 300.0, 0.0));
    e.input(ToolInput::Hover { px: at });
    e.input(ToolInput::Click { px: at, mods: Modifiers::default() });
    e.input(ToolInput::Click { px: at, mods: Modifiers::default() });
    e.input(ToolInput::DoubleClick { px: at, mods: Modifiers::default() });
    assert_eq!(count(&e), before, "closed, not pulled");
    assert!(e.status().contains("hauteur"), "{}", e.status());
    // A double-click now (on the height) takes the last height again.
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(100.0, 50.0, 0.0)) });
    e.input(ToolInput::DoubleClick { px: at, mods: Modifiers::default() });
    let (_, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!((hi.z - 77.0).abs() < 1e-6, "{hi}");
}

#[test]
fn prism_takes_a_whole_face_arcs_included() {
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    draw_box(&mut e, DVec3::ZERO, DVec3::new(300.0, 200.0, 0.0), "150");
    let disc = Primitive::Cylinder { origin: DVec3::new(600.0, 600.0, 0.0), radius: 50.0, height: 30.0 };
    e.apply(Operation::AddPrimitive { context: root, primitive: disc, placement: glam::DAffine3::IDENTITY, material: None }).unwrap();
    let hover = |e: &mut Editor, p: DVec3| e.input(ToolInput::Hover { px: px(e, p) });
    e.set_tool(ToolKind::Prism);
    // The disc: two half circles, the height next.
    e.set_copy(true);
    hover(&mut e, DVec3::new(610.0, 590.0, 30.0));
    key(&mut e, Key::Shift);
    e.set_copy(false);
    assert!(e.status().contains("hauteur"), "{}", e.status());
    hover(&mut e, DVec3::new(650.0, 600.0, 30.0));
    e.input(ToolInput::Vcb("10".into()));
    let (lo, hi) = mesh_bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(near(lo, DVec3::new(550.0, 550.0, 30.0)) && near(hi, DVec3::new(650.0, 650.0, 40.0)), "{lo} {hi}");
    assert_eq!(part_topology(&e).0, 4, "two half cylinders and two caps");
    // The box's top: its outline is the profile, the height next.
    e.set_copy(true);
    hover(&mut e, DVec3::new(100.0, 50.0, 150.0));
    key(&mut e, Key::Shift);
    e.set_copy(false);
    hover(&mut e, DVec3::new(300.0, 200.0, 150.0));
    e.input(ToolInput::Vcb("10".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(close(lo, DVec3::new(0.0, 0.0, 150.0)) && close(hi, DVec3::new(300.0, 200.0, 160.0)), "{lo} {hi}");
}

#[test]
fn prism_on_an_upright_face() {
    let mut e = top_editor(Document::new());
    draw_box(&mut e, DVec3::ZERO, DVec3::new(300.0, 200.0, 0.0), "150");
    e.set_viewport_size(DVec2::new(1000.0, 1000.0));
    e.camera.set_standard_view(
        StandardView::Front,
        glam::DMat3::IDENTITY,
        Some((DVec3::splat(-1000.0), DVec3::splat(1000.0))),
        crate::camera::ViewArea::full(e.viewport_size()),
    );
    e.set_tool(ToolKind::Prism);
    // A triangle on the front face (y = 0), pulled out towards the eye by 20.
    // (Away from the box's edges: 0.5 px per mm here.)
    for p in [DVec3::new(60.0, 0.0, 50.0), DVec3::new(240.0, 0.0, 50.0), DVec3::new(150.0, 0.0, 100.0)] {
        click(&mut e, p);
    }
    key(&mut e, Key::Enter);
    e.input(ToolInput::Vcb("20".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(close(lo, DVec3::new(60.0, -20.0, 50.0)) && close(hi, DVec3::new(240.0, 0.0, 100.0)), "{lo} {hi}");
}

#[test]
fn prism_second_corner_straight_above_the_first() {
    let mut e = top_editor(Document::new());
    let area = crate::camera::ViewArea::full(e.viewport_size());
    e.camera.set_standard_view(StandardView::Iso, glam::DMat3::IDENTITY, Some((DVec3::splat(-500.0), DVec3::splat(500.0))), area);
    e.set_tool(ToolKind::Prism);
    // On the ground, then up the blue axis: the second corner is taken (an upright side), the
    // outline drawn in the upright plane the third corner gives.
    click(&mut e, DVec3::ZERO);
    click(&mut e, DVec3::new(0.0, 0.0, 300.0));
    click(&mut e, DVec3::new(200.0, 0.0, 300.0));
    click(&mut e, DVec3::new(200.0, 0.0, 0.0));
    key(&mut e, Key::Enter);
    assert!(e.status().contains("hauteur"), "{}", e.status());
    e.input(ToolInput::Vcb("18".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!((hi - lo - DVec3::new(200.0, 18.0, 300.0)).length() < 1e-6, "{lo} {hi}");
}

#[test]
fn prism_offset_moves_the_outline_made() {
    let mut e = top_editor(Document::new());
    e.set_tool(ToolKind::Prism);
    // `5x` sets the offset, at any step: the outline made grows by 5 on every side.
    e.input(ToolInput::Vcb("5x".into()));
    corners(&mut e, &[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)]);
    key(&mut e, Key::Enter);
    e.input(ToolInput::Vcb("10".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(close(lo, DVec3::new(-5.0, -5.0, 0.0)) && close(hi, DVec3::new(105.0, 105.0, 10.0)), "{lo} {hi}");

    // A U whose notch (40 wide) an outward offset of 25 closes: refused, the height step stays.
    e.set_tool_option("offset", OptionValue::Length(25.0));
    let u = [(300.0, 0.0), (420.0, 0.0), (420.0, 100.0), (380.0, 100.0), (380.0, 50.0), (340.0, 50.0), (340.0, 100.0), (300.0, 100.0)];
    corners(&mut e, &u);
    key(&mut e, Key::Enter);
    e.input(ToolInput::Vcb("10".into()));
    assert!(e.status().contains("croiser"), "{}", e.status());
    // Inward past its middle: nothing left.
    e.input(ToolInput::Vcb("-80x".into()));
    e.input(ToolInput::Vcb("10".into()));
    assert!(e.status().contains("ne laisse rien"), "{}", e.status());
    // Inward by 5: made.
    e.input(ToolInput::Vcb("-5x".into()));
    e.input(ToolInput::Vcb("10".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(close(lo, DVec3::new(305.0, 5.0, 0.0)) && close(hi, DVec3::new(415.0, 95.0, 10.0)), "{lo} {hi}");
}

/// World bounds of the display mesh of the part an occurrence path names: arcs included (the
/// part's vertices are only its corners).
fn mesh_bounds_of(e: &Editor, path: &EntityPath) -> (DVec3, DVec3) {
    let (_, _, t) = e.document().resolve_part(path).unwrap();
    let def = e.document().resolve(path).unwrap().component;
    let mut lo = DVec3::INFINITY;
    let mut hi = DVec3::NEG_INFINITY;
    for part in &e.scene().definition_mesh(def).unwrap().parts {
        let m = &part.mesh;
        for p in &m.positions {
            let w = t.transform_point3(m.origin + DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64));
            lo = lo.min(w);
            hi = hi.max(w);
        }
    }
    (lo, hi)
}

/// Is there a point of the last part drawn's display mesh near `p` (world)?
fn mesh_has(e: &Editor, p: DVec3) -> bool {
    let path = EntityPath::root(drawn(e));
    let (_, _, t) = e.document().resolve_part(&path).unwrap();
    let def = e.document().resolve(&path).unwrap().component;
    e.scene().definition_mesh(def).unwrap().parts.iter().any(|part| {
        let m = &part.mesh;
        m.positions.iter().any(|q| t.transform_point3(m.origin + DVec3::new(q[0] as f64, q[1] as f64, q[2] as f64)).distance(p) < 0.5)
    })
}

/// Near enough for a display mesh (`f32`, arcs as chords).
fn near(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < 0.05
}

fn part_topology(e: &Editor) -> (usize, usize, usize) {
    let (_, part, _) = e.document().resolve_part(&EntityPath::root(drawn(e))).unwrap();
    part.shape().topology_counts()
}

fn trace(e: &Editor) -> Option<OptionValue> {
    e.tool_options().into_iter().find(|o| o.key == "trace").map(|o| o.value)
}

#[test]
fn prism_tangent_arc_and_back_to_lines() {
    let mut e = top_editor(Document::new());
    e.set_tool(ToolKind::Prism);
    corners(&mut e, &[(0.0, 0.0), (200.0, 0.0)]);
    // A quarter circle tangent to the bottom side, to (300, 100).
    e.set_tool_option("trace", OptionValue::Choice("tangent".into()));
    click(&mut e, DVec3::new(300.0, 100.0, 0.0));
    assert_eq!(trace(&e), Some(OptionValue::Choice("tangent".into())), "the mode stays");
    e.set_tool_option("trace", OptionValue::Choice("line".into()));
    corners(&mut e, &[(300.0, 300.0), (0.0, 300.0), (0.0, 0.0)]);
    e.input(ToolInput::Vcb("10".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(close(lo, DVec3::ZERO) && close(hi, DVec3::new(300.0, 300.0, 10.0)), "{lo} {hi}");
    // Five sides (one an arc) and two caps.
    assert_eq!(part_topology(&e), (7, 15, 10));
    // The quarter circle about (200, 100), tangent to the bottom: through its middle
    // (200 + 100·sin 45°, 100 − 100·cos 45°), not bulging inwards.
    let h = 100.0 * std::f64::consts::FRAC_1_SQRT_2;
    assert!(mesh_has(&e, DVec3::new(200.0 + h, 100.0 - h, 0.0)));
    assert!(!mesh_has(&e, DVec3::new(300.0 - h, h, 0.0)));
    // Each new outline starts with lines.
    corners(&mut e, &[(500.0, 0.0)]);
    assert_eq!(trace(&e), Some(OptionValue::Choice("line".into())));
}

#[test]
fn prism_rest_on_the_last_corner_switches_to_tangent_arcs() {
    let mut e = top_editor(Document::new());
    e.set_tool(ToolKind::Prism);
    e.tick(10.0);
    corners(&mut e, &[(0.0, 0.0), (200.0, 0.0)]);
    let on = px(&e, DVec3::new(200.0, 0.0, 0.0));
    // Resting right after the click switches nothing: the cursor must leave first.
    e.input(ToolInput::Hover { px: on });
    e.tick(11.0);
    assert_eq!(trace(&e), Some(OptionValue::Choice("line".into())));
    // Away (40 px), back onto it, still for 0.35 s.
    e.input(ToolInput::Hover { px: on + DVec2::new(40.0, 0.0) });
    e.tick(12.0);
    e.input(ToolInput::Hover { px: on + DVec2::new(1.0, 0.0) });
    assert!(e.tick(12.1), "waiting: called again");
    assert_eq!(trace(&e), Some(OptionValue::Choice("line".into())));
    e.tick(12.5);
    assert_eq!(trace(&e), Some(OptionValue::Choice("tangent".into())));
    // And back the same way.
    e.input(ToolInput::Hover { px: on + DVec2::new(0.0, 40.0) });
    e.tick(13.0);
    e.input(ToolInput::Hover { px: on });
    e.tick(13.1);
    e.tick(13.5);
    assert_eq!(trace(&e), Some(OptionValue::Choice("line".into())));
}

#[test]
fn prism_arc_by_two_points_its_end_then_its_bulge() {
    let mut e = top_editor(Document::new());
    e.set_tool(ToolKind::Prism);
    corners(&mut e, &[(0.0, 0.0), (200.0, 0.0)]);
    e.set_tool_option("trace", OptionValue::Choice("arc".into()));
    // The end (200, 200), then the bulge out to the right: a half circle (snapped).
    click(&mut e, DVec3::new(200.0, 200.0, 0.0));
    assert!(e.status().contains("bombé"), "{}", e.status());
    // Esc drops the arc under way, not the corners.
    key(&mut e, Key::Escape);
    click(&mut e, DVec3::new(200.0, 200.0, 0.0));
    click(&mut e, DVec3::new(302.0, 100.0, 0.0));
    e.set_tool_option("trace", OptionValue::Choice("line".into()));
    click(&mut e, DVec3::new(0.0, 200.0, 0.0));
    key(&mut e, Key::Enter);
    e.input(ToolInput::Vcb("10".into()));
    // The half circle bulges out to x = 300 (the part's corners stop at 200).
    let (lo, hi) = mesh_bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(near(lo, DVec3::ZERO) && near(hi, DVec3::new(300.0, 200.0, 10.0)), "{lo} {hi}");

    // Closing with an arc: Enter sets the end (the first corner), the bulge typed.
    e.set_tool(ToolKind::Prism);
    corners(&mut e, &[(500.0, 0.0), (700.0, 0.0)]);
    e.set_tool_option("trace", OptionValue::Choice("arc".into()));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(600.0, -50.0, 0.0)) });
    key(&mut e, Key::Enter);
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(600.0, 50.0, 0.0)) });
    e.input(ToolInput::Vcb("100".into()));
    e.input(ToolInput::Vcb("10".into()));
    // A half disc above the bottom side.
    let (lo, hi) = mesh_bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(near(lo, DVec3::new(500.0, 0.0, 0.0)) && near(hi, DVec3::new(700.0, 100.0, 10.0)), "{lo} {hi}");
}

#[test]
fn prism_fillet_rounds_the_corners_made() {
    let mut e = top_editor(Document::new());
    e.set_tool(ToolKind::Prism);
    // `10r` sets the fillet, `5x` the offset: the board made is 110 × 60, its corners R10.
    e.input(ToolInput::Vcb("10r".into()));
    e.input(ToolInput::Vcb("5x".into()));
    let length = |e: &Editor, k: &str| e.tool_options().into_iter().find(|o| o.key == k).map(|o| o.value);
    assert_eq!(length(&e, "fillet"), Some(OptionValue::Length(10.0)));
    corners(&mut e, &[(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)]);
    key(&mut e, Key::Enter);
    e.input(ToolInput::Vcb("18".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(close(lo, DVec3::new(-5.0, -5.0, 0.0)) && close(hi, DVec3::new(105.0, 55.0, 18.0)), "{lo} {hi}");
    assert_eq!(part_topology(&e), (10, 24, 16));
    // The fillets are radii on the sharp corners of the part's profile.
    let b = blank_profile(&e, drawn(&e));
    assert_eq!(b.profile.len(), 4);
    assert!(b.profile.iter().all(|c| c.fillet == 10.0), "{b:?}");
}

#[test]
fn box_fillet_rounds_the_corners_made() {
    let mut e = top_editor(Document::new());
    e.set_tool(ToolKind::Box);
    // `10r` sets the box's fillet (its own: the prism's stays none), `5x` the offset: the board
    // made is 110 × 60, a prism with its four corners R10.
    e.input(ToolInput::Vcb("10r".into()));
    e.input(ToolInput::Vcb("5x".into()));
    let length = |e: &Editor, k: &str| e.tool_options().into_iter().find(|o| o.key == k).map(|o| o.value);
    assert_eq!(length(&e, "fillet"), Some(OptionValue::Length(10.0)));
    let (lo, hi) = box_bounds(&mut e, DVec3::ZERO, DVec3::new(100.0, 50.0, 0.0), "18");
    assert!(close(lo, DVec3::new(-5.0, -5.0, 0.0)) && close(hi, DVec3::new(105.0, 55.0, 18.0)), "{lo} {hi}");
    assert_eq!(part_topology(&e), (10, 24, 16));
    let b = blank_profile(&e, drawn(&e));
    assert_eq!(b.profile.len(), 4);
    assert!(b.profile.iter().all(|c| c.fillet == 10.0), "{b:?}");

    // Half the shorter side: a stadium. Over it: refused, said, the height step kept.
    e.input(ToolInput::Vcb("0x".into()));
    e.input(ToolInput::Vcb("25r".into()));
    box_bounds(&mut e, DVec3::new(200.0, 0.0, 0.0), DVec3::new(300.0, 50.0, 0.0), "18");
    let b = blank_profile(&e, drawn(&e));
    assert!(b.profile.iter().all(|c| (c.fillet - 25.0).abs() < 1e-3), "{b:?}");
    e.input(ToolInput::Vcb("26r".into()));
    let before = drawn(&e);
    click(&mut e, DVec3::new(400.0, 0.0, 0.0));
    e.input(ToolInput::Vcb("100;50;18".into()));
    assert_eq!(drawn(&e), before);
    assert_eq!(e.status(), "Congé trop grand : au plus la moitié du petit côté de la base faite");
    assert_eq!(e.vcb().map(|(l, _)| l), Some("Hauteur".into()), "the height step stays");
    e.input(ToolInput::Vcb("5r".into()));
    e.input(ToolInput::Vcb("18".into()));
    assert_ne!(drawn(&e), before);
    let (lo, hi) = mesh_bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(near(lo, DVec3::new(400.0, 0.0, 0.0)) && near(hi, DVec3::new(500.0, 50.0, 18.0)), "{lo} {hi}");

    // No fillet: a plain box again.
    e.input(ToolInput::Vcb("0r".into()));
    box_bounds(&mut e, DVec3::new(600.0, 0.0, 0.0), DVec3::new(700.0, 50.0, 0.0), "18");
    assert_eq!(part_topology(&e), (6, 12, 8));
    e.set_tool(ToolKind::Prism);
    assert_eq!(length(&e, "fillet"), Some(OptionValue::Length(0.0)));
}

#[test]
fn profile_fillets_follow_a_moved_corner() {
    let mut e = top_editor(Document::new());
    e.set_tool(ToolKind::Prism);
    e.input(ToolInput::Vcb("10r".into()));
    corners(&mut e, &[(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)]);
    key(&mut e, Key::Enter);
    e.input(ToolInput::Vcb("18".into()));
    let id = drawn(&e);
    e.set_tool(ToolKind::Select);
    e.enter(id);
    e.set_tool(ToolKind::Profile);
    // The rounded corner's grip is the sharp corner, off the outline, its sides dashed to it.
    // Moved, the board stays a rectangle with its four fillets: 200 × 50.
    click(&mut e, DVec3::new(100.0, 50.0, 18.0));
    click(&mut e, DVec3::new(200.0, 50.0, 18.0));
    let b = blank_profile(&e, id);
    assert!(b.profile.iter().all(|c| c.fillet == 10.0), "{b:?}");
    assert!(b.profile.iter().any(|c| c.p.distance(glam::DVec2::new(200.0, 50.0)) < 1e-3), "{b:?}");
    let (_, part, _) = e.document().resolve_part(&EntityPath::root(id)).unwrap();
    assert_eq!(part.shape().topology_counts(), (10, 24, 16));
    let (_, hi) = mesh_bounds_of(&e, &EntityPath::root(id));
    assert!((hi.x - 200.0).abs() < 1e-3, "{hi}");
    // The bottom right corner moved alone (free) back to (100, 0): the top right corner turns
    // acute, its fillet, still tangent and R10, sitting back from it — its centre r / sin(13.3°)
    // away, the outline reaching x ≈ 167.6.
    key(&mut e, Key::Copy);
    click(&mut e, DVec3::new(200.0, 0.0, 18.0));
    click(&mut e, DVec3::new(100.0, 0.0, 18.0));
    let (_, part, _) = e.document().resolve_part(&EntityPath::root(id)).unwrap();
    assert_eq!(part.shape().topology_counts(), (10, 24, 16));
    let (_, hi) = mesh_bounds_of(&e, &EntityPath::root(id));
    assert!(hi.x > 165.0 && hi.x < 170.0, "{hi}");
}

// ---- Profile (docs/tools/prism.md, step 3) ------------------------------------------------

/// A board 600 × 300 × 18 drawn at the origin, its part open, the profile tool out.
fn board_profile_editor() -> (Editor, Id) {
    let mut e = top_editor(Document::new());
    let id = draw_box(&mut e, DVec3::ZERO, DVec3::new(600.0, 300.0, 0.0), "18");
    e.enter(id);
    e.set_tool(ToolKind::Profile);
    (e, id)
}

fn blank_profile(e: &Editor, id: Id) -> skraft_model::BodyProfile {
    let p = e.document().resolve_part(&EntityPath::root(id)).unwrap().1;
    p.profile_of(p.profiled().unwrap()).cloned().unwrap()
}

#[test]
fn profile_corner_moved_side_split() {
    let (mut e, id) = board_profile_editor();
    // The far corner taken and dropped 100 further along red.
    click(&mut e, DVec3::new(600.0, 300.0, 18.0));
    assert!(e.vcb().is_some_and(|(l, _)| l == "Distance"), "{:?}", e.vcb());
    click(&mut e, DVec3::new(700.0, 300.0, 18.0));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(id));
    assert!(close(lo, DVec3::ZERO) && close(hi, DVec3::new(700.0, 300.0, 18.0)), "{lo} {hi}");
    assert_eq!(blank_profile(&e, id).profile.len(), 4);

    // The bottom side's middle (350, 0): the side moved down 100 (typed), still a rectangle.
    click(&mut e, DVec3::new(350.0, 0.0, 18.0));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(350.0, -50.0, 18.0)) });
    e.input(ToolInput::Vcb("100".into()));
    assert_eq!(blank_profile(&e, id).profile.len(), 4);
    let (lo, hi) = bounds_of(&e, &EntityPath::root(id));
    assert!(close(lo, DVec3::new(0.0, -100.0, 0.0)) && close(hi, DVec3::new(700.0, 300.0, 18.0)), "{lo} {hi}");

    // Free (the copy modifier tapped): the bottom side's middle (350, -100) inserts a corner,
    // pulled down 100 more; free drops once done.
    key(&mut e, Key::Copy);
    assert!(e.status().contains("(libre)"), "{}", e.status());
    click(&mut e, DVec3::new(350.0, -100.0, 18.0));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(350.0, -150.0, 18.0)) });
    e.input(ToolInput::Vcb("100".into()));
    assert_eq!(blank_profile(&e, id).profile.len(), 5);
    let (lo, _) = bounds_of(&e, &EntityPath::root(id));
    assert!(close(lo, DVec3::new(0.0, -200.0, 0.0)), "{lo}");
    assert!(!e.status().contains("(libre)"), "{}", e.status());

    // Esc drops a gesture: nothing changes.
    let before = blank_profile(&e, id);
    click(&mut e, DVec3::new(0.0, 300.0, 18.0));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(-200.0, 300.0, 18.0)) });
    key(&mut e, Key::Escape);
    assert_eq!(blank_profile(&e, id), before);
    // One undo per gesture: the corner inserted taken away.
    e.undo();
    assert_eq!(blank_profile(&e, id).profile.len(), 4);
}

#[test]
fn profile_corner_keeps_the_sides_directions_unless_free() {
    let (mut e, id) = board_profile_editor();
    // The far corner dragged down and right: its sides translated, still a rectangle.
    click(&mut e, DVec3::new(600.0, 300.0, 0.0));
    click(&mut e, DVec3::new(650.0, 200.0, 0.0));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(id));
    assert!(close(lo, DVec3::ZERO) && close(hi, DVec3::new(650.0, 200.0, 18.0)), "{lo} {hi}");
    let (_, part, _) = e.document().resolve_part(&EntityPath::root(id)).unwrap();
    assert_eq!(part.shape().topology_counts(), (6, 12, 8));
    // The copy modifier tapped while moving: the corner alone, its neighbours left in place.
    click(&mut e, DVec3::new(650.0, 200.0, 0.0));
    key(&mut e, Key::Copy);
    click(&mut e, DVec3::new(700.0, 250.0, 0.0));
    let b = blank_profile(&e, id);
    assert!(b.profile.iter().any(|c| c.p.distance(glam::DVec2::new(650.0, 0.0)) < 1e-3), "{b:?}");
    assert!(b.profile.iter().any(|c| c.p.distance(glam::DVec2::new(0.0, 200.0)) < 1e-3), "{b:?}");
    assert!(b.profile.iter().any(|c| c.p.distance(glam::DVec2::new(700.0, 250.0)) < 1e-3), "{b:?}");
}

#[test]
fn profile_grips_say_how_they_move() {
    use crate::tools::PointShape;
    let (mut e, _) = board_profile_editor();
    let shape_at = |e: &Editor, p: DVec3| e.overlay_points().into_iter().find(|g| (g.at - p).length() < 1e-3).map(|g| g.shape);
    // Kept: a corner a square, a side's middle a bar along the side.
    assert_eq!(shape_at(&e, DVec3::new(600.0, 300.0, 0.0)), Some(PointShape::Square));
    let Some(PointShape::Bar { along }) = shape_at(&e, DVec3::new(300.0, 0.0, 0.0)) else { panic!() };
    assert!(along.normalize().cross(DVec3::X).length() < 1e-9 && along.length() > 1.0, "{along}");
    let Some(PointShape::Bar { along }) = shape_at(&e, DVec3::new(600.0, 150.0, 0.0)) else { panic!() };
    assert!(along.normalize().cross(DVec3::Y).length() < 1e-9, "{along}");
    // Free: round.
    key(&mut e, Key::Copy);
    assert_eq!(shape_at(&e, DVec3::new(600.0, 300.0, 0.0)), Some(PointShape::Round));
    assert_eq!(shape_at(&e, DVec3::new(300.0, 0.0, 0.0)), Some(PointShape::Round));
}

#[test]
fn profile_corner_deleted_crossing_refused_disc_radius() {
    let (mut e, id) = board_profile_editor();
    // A corner dragged alone (free) across the bottom side: refused, said; nothing changes.
    let before = blank_profile(&e, id);
    key(&mut e, Key::Copy);
    let from = px(&e, DVec3::new(600.0, 300.0, 18.0));
    e.input(ToolInput::Hover { px: from });
    e.input(ToolInput::DragStart { px: from, mods: Modifiers::default() });
    let to = px(&e, DVec3::new(300.0, -100.0, 18.0));
    e.input(ToolInput::Drag { px: to });
    e.input(ToolInput::DragEnd { px: to, mods: Modifiers::default() });
    assert!(e.status().contains("croiserait"), "{}", e.status());
    assert_eq!(blank_profile(&e, id), before);
    key(&mut e, Key::Escape);
    // Delete over a corner: a triangle.
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(600.0, 300.0, 18.0)) });
    key(&mut e, Key::Delete);
    assert_eq!(blank_profile(&e, id).profile.len(), 3);
    let (_, part, _) = e.document().resolve_part(&EntityPath::root(id)).unwrap();
    assert_eq!(part.shape().topology_counts(), (5, 9, 6));

    // A cylinder's disc: a single grip, its radius — dragged, then typed, it stays a disc.
    // Out of the body open (its profile), then of the part.
    e.exit();
    assert_eq!(e.tool_kind(), ToolKind::Select);
    assert_eq!(e.open_part(), Some(id));
    e.exit();
    assert_eq!(e.open_part(), None);
    e.set_tool(ToolKind::Cylinder);
    click(&mut e, DVec3::new(-500.0, -500.0, 0.0));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(-400.0, -500.0, 0.0)) });
    e.input(ToolInput::Vcb("100".into()));
    e.input(ToolInput::Vcb("10".into()));
    let rod = drawn(&e);
    e.set_tool(ToolKind::Select);
    e.enter(rod);
    e.set_tool(ToolKind::Profile);
    let b = blank_profile(&e, rod);
    let (_, _, t) = e.document().resolve_part(&EntityPath::root(rod)).unwrap();
    let grip = t.transform_point3(b.profile[0].p.extend(10.0));
    let centre = t.transform_point3(DVec3::new(0.0, 0.0, 10.0));
    click(&mut e, grip);
    assert!(e.vcb().is_some_and(|(l, _)| l == "Rayon"), "{:?}", e.vcb());
    click(&mut e, centre + (grip - centre) * 1.5);
    let (_, r) = skraft_model::kernel::profile::circle(&blank_profile(&e, rod).profile).expect("a disc still");
    assert!((r - 150.0).abs() < 0.5, "{r}");
    click(&mut e, centre + (grip - centre) * 1.5);
    e.input(ToolInput::Vcb("80".into()));
    let (_, r) = skraft_model::kernel::profile::circle(&blank_profile(&e, rod).profile).expect("a disc still");
    assert!((r - 80.0).abs() < 1e-9, "{r}");
    let (lo, hi) = mesh_bounds_of(&e, &EntityPath::root(rod));
    assert!(((hi - lo).x - 160.0).abs() < 1.0 && ((hi - lo).y - 160.0).abs() < 1.0, "{lo} {hi}");
}

#[test]
fn profile_fillet_grip_rounds_a_corner_and_makes_it_sharp_again() {
    use skraft_model::kernel::profile;
    let (mut e, id) = board_profile_editor();
    let fillet_grips = |e: &Editor| e.overlay_points().into_iter().filter(|p| p.radius == 4.0 && p.shape == PointShape::Round).count();
    assert_eq!(fillet_grips(&e), 0, "no fillet grip before a corner is hovered");
    // Hovering the far corner shows its fillet grip, just inside on the bisector.
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(600.0, 300.0, 18.0)) });
    let grip = e.overlay_points().into_iter().find(|p| p.radius == 4.0 && p.shape == PointShape::Round).expect("a fillet grip").at;
    assert!(grip.x < 600.0 && grip.y < 300.0 && ((600.0 - grip.x) - (300.0 - grip.y)).abs() < 1e-3, "{grip}");
    // Taken (the corner's hover kept), the radius typed: 20.
    click(&mut e, grip);
    assert!(e.vcb().is_some_and(|(l, _)| l == "Congé"), "{:?}", e.vcb());
    e.input(ToolInput::Vcb("20".into()));
    assert_eq!(blank_profile(&e, id).profile[2].fillet, 20.0);
    let (_, part, _) = e.document().resolve_part(&EntityPath::root(id)).unwrap();
    assert_eq!(part.shape().topology_counts(), (7, 15, 10));
    // Too big for the sides: refused, said.
    let m = profile::fillet_middle(&blank_profile(&e, id).profile, 2, 20.0).unwrap().extend(18.0);
    assert_eq!(fillet_grips(&e), 1, "a rounded corner's grip always shows");
    click(&mut e, m);
    e.input(ToolInput::Vcb("400".into()));
    assert!(e.status().contains("trop grand"), "{}", e.status());
    // Brought back onto the corner: sharp again.
    click(&mut e, DVec3::new(600.0, 300.0, 18.0));
    assert_eq!(blank_profile(&e, id).profile[2].fillet, 0.0);
    // Dragged out along the bisector: the radius of the arc through the cursor.
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(600.0, 300.0, 18.0)) });
    click(&mut e, grip);
    let d = 100.0 * (2f64.sqrt() - 1.0);
    click(&mut e, DVec3::new(600.0, 300.0, 18.0) - DVec3::new(1.0, 1.0, 0.0).normalize() * d);
    assert!((blank_profile(&e, id).profile[2].fillet - 100.0).abs() < 0.1, "{:?}", blank_profile(&e, id));
    // Delete over it: sharp.
    let m = profile::fillet_middle(&blank_profile(&e, id).profile, 2, blank_profile(&e, id).profile[2].fillet).unwrap().extend(18.0);
    e.input(ToolInput::Hover { px: px(&e, m) });
    key(&mut e, Key::Delete);
    assert_eq!(blank_profile(&e, id).profile[2].fillet, 0.0);
}

#[test]
fn profile_shows_where_it_lies_whatever_the_view() {
    let (e, _) = board_profile_editor();
    // No cursor yet: seen from above too, on the bottom cap (z = 0), where the profile lies.
    let lines = e.preview().drawing.lines;
    assert!(!lines.is_empty() && lines.iter().all(|l| l.a.z.abs() < 1e-9), "{:?}", lines.first());
}

// ---- copy and paste (docs/tools/copy-paste.md) ---------------------------------------------------------

#[test]
fn copy_and_paste_in_place_within_and_across_documents() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 50.0, 0.0), "20");
    assert!(e.can_copy());
    let clip = e.copy().unwrap();
    let parts = e.document().components().count();

    // The same document: one more occurrence of the same part, where it was, selected.
    assert!(e.can_paste(&clip));
    e.paste_in_place(&clip, true);
    let pasted = e.selection().to_vec();
    assert_eq!(pasted.len(), 1);
    assert_ne!(pasted[0], a);
    assert_eq!(e.document().components().count(), parts);
    assert!(close(bounds_of(&e, &EntityPath::root(pasted[0])).0, bounds_of(&e, &EntityPath::root(a)).0));

    // Another document: a part of its own, at the same place.
    let mut other = top_editor(Document::new());
    other.paste_in_place(&clip, false);
    assert_eq!(other.selection().len(), 1);
    assert_eq!(other.document().components().count(), 1);
    assert!(close(bounds_of(&other, &EntityPath::root(other.selection()[0])).1, DVec3::new(100.0, 50.0, 20.0)));

    // Cut: copied and erased, one step; pasted back, it is there again.
    e.select(vec![a]);
    let cut = e.cut().unwrap();
    assert!(e.document().resolve(&EntityPath::root(a)).is_err());
    assert!(e.selection().is_empty());
    e.paste_in_place(&cut, true);
    assert_eq!(e.document().root_assembly().occurrences().len(), 2);
    // Nothing selected: nothing to copy.
    e.select(vec![]);
    assert!(!e.can_copy() && e.copy().is_none());
}

#[test]
fn copied_in_an_assembly_pasted_in_place_at_the_root() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::new(200.0, 0.0, 0.0), DVec3::new(300.0, 50.0, 0.0), "20");
    e.make_assembly();
    let g = e.selection()[0];
    e.enter(g);
    e.select(vec![a]);
    let clip = e.copy().unwrap();
    let world = bounds_of(&e, &EntityPath::new(vec![g], a));
    e.exit();
    e.paste_in_place(&clip, true);
    let pasted = e.selection()[0];
    let at = bounds_of(&e, &EntityPath::root(pasted));
    assert!(close(at.0, world.0) && close(at.1, world.1), "{at:?} {world:?}");
}

#[test]
fn machinings_paste_into_an_open_part() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(200.0, 50.0, 0.0), "18");
    let b = draw_box(&mut e, DVec3::new(0.0, 100.0, 0.0), DVec3::new(200.0, 150.0, 0.0), "18");
    // A hole in the first board, as a boolean would make it.
    let root = e.document().root();
    let drill = Primitive::Cylinder { origin: DVec3::new(20.0, 25.0, -5.0), radius: 4.0, height: 30.0 };
    let hole = e
        .apply(Operation::AddPrimitive { context: root, primitive: drill, placement: glam::DAffine3::IDENTITY, material: None })
        .unwrap()
        .unwrap();
    let tool = hole.created[0];
    e.apply(Operation::boolean(EntityPath::root(a), EntityPath::root(tool), Boolean::Difference, Default::default())).unwrap();

    // Copied in the open part, its machinings shown.
    e.enter(a);
    let holes = e.document().resolve_part(&EntityPath::root(a)).unwrap().1.machinings().iter().map(|m| m.id).collect();
    e.select(holes);
    let clip = e.copy().unwrap();
    assert!(matches!(clip.content, Content::Machinings(_)));
    let (_, part, t) = e.document().resolve_part(&EntityPath::root(a)).unwrap();
    let world = (t * part.machinings()[0].placement).translation;
    e.exit();
    e.enter(b);
    assert!(e.can_paste(&clip));
    // In place: where it was in world space, not at the same place in the other part's axes.
    e.paste_in_place(&clip, true);
    let (_, part, t) = e.document().resolve_part(&EntityPath::root(b)).unwrap();
    assert_eq!(part.machinings().len(), 1);
    assert!(close((t * part.machinings()[0].placement).translation, world), "{:?}", t * part.machinings()[0].placement);
    assert_eq!(e.selection(), &[part.machinings()[0].id]);

    // At the cursor: carried from the low corner, put down 50 further along red.
    e.paste(&clip, true);
    assert!(e.carrying());
    e.input(ToolInput::Key { key: Key::Right, mods: Modifiers::default() });
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(120.0, 125.0, 0.0)) });
    e.input(ToolInput::Vcb("50".into()));
    assert!(!e.carrying());
    let (_, part, t) = e.document().resolve_part(&EntityPath::root(b)).unwrap();
    assert_eq!(part.machinings().len(), 2);
    let at = (t * part.machinings()[1].placement).translation;
    assert!(close(at, world + DVec3::new(50.0, 0.0, 0.0)), "{at:?}");
}

#[test]
fn bodies_pasted_in_place_into_a_part_placed_elsewhere_keep_their_world_place() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::new(100.0, 0.0, 0.0), DVec3::new(300.0, 50.0, 0.0), "18");
    let b = draw_box(&mut e, DVec3::new(0.0, 400.0, 0.0), DVec3::new(200.0, 450.0, 0.0), "18");
    // The second board turned a quarter about blue and lifted: its axes are not the first's.
    let root = e.document().root();
    let turn =
        glam::DAffine3::from_translation(DVec3::new(-500.0, 0.0, 300.0)) * glam::DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_2);
    e.apply(Operation::Transform { context: root, entities: vec![b], transform: turn }).unwrap();
    e.enter(a);
    let blank = e.document().resolve_part(&EntityPath::root(a)).unwrap().1.bodies[0].id;
    e.select(vec![blank]);
    let clip = e.copy().unwrap();
    e.exit();
    let world = bounds_of(&e, &EntityPath::root(a));
    e.enter(b);
    e.paste_in_place(&clip, true);
    let (_, part, t) = e.document().resolve_part(&EntityPath::root(b)).unwrap();
    let body = part.bodies.iter().find(|x| e.selection() == [x.id]).unwrap();
    let def = part.def_of(body);
    let (lo, hi) = def.solid.vertex_bounds().unwrap();
    let at = crate::scene::transform_bounds(&(t * body.placement), (lo, hi));
    assert!(close(at.0, world.0) && close(at.1, world.1), "{at:?} {world:?}");
}

#[test]
fn a_part_pasted_into_an_open_part_becomes_a_group_of_its_bodies() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(200.0, 50.0, 0.0), "18");
    let b = draw_box(&mut e, DVec3::new(0.0, 100.0, 0.0), DVec3::new(200.0, 150.0, 0.0), "18");
    let root = e.document().root();
    let drill = Primitive::Cylinder { origin: DVec3::new(20.0, 25.0, -5.0), radius: 4.0, height: 30.0 };
    let tool = e
        .apply(Operation::AddPrimitive { context: root, primitive: drill, placement: glam::DAffine3::IDENTITY, material: None })
        .unwrap()
        .unwrap();
    e.apply(Operation::boolean(EntityPath::root(a), EntityPath::root(tool.created[0]), Boolean::Difference, Default::default())).unwrap();
    e.take_notices();

    // The drilled board copied, pasted into the other one: one positive body, a group of its
    // blank and its hole, at its world place, selected and shown; nothing
    // lost, nothing to say.
    e.select(vec![a]);
    let clip = e.copy().unwrap();
    e.enter(b);
    assert!(e.can_paste(&clip));
    e.paste_in_place(&clip, true);
    let (_, part, t) = e.document().resolve_part(&EntityPath::root(b)).unwrap();
    assert_eq!(part.positives().count(), 2);
    let body = part.bodies.iter().find(|x| e.selection() == [x.id]).unwrap();
    assert!(part.profile_of(body).is_none());
    assert_eq!(part.def_of(body).children().len(), 2);
    assert!(close((t * body.placement).translation, DVec3::ZERO), "{:?}", t * body.placement);
    assert!(e.take_notices().is_empty());
    // Grouped and ungrouped by the commands, the group selected.
    assert!(e.can_explode());
    e.explode();
    let (_, part, _) = e.document().resolve_part(&EntityPath::root(b)).unwrap();
    assert_eq!((part.positives().count(), part.negatives().count()), (2, 1));
    assert_eq!(e.selection().len(), 2);
    assert!(!e.can_explode() && e.can_group_bodies());
    e.make_assembly();
    let (_, part, _) = e.document().resolve_part(&EntityPath::root(b)).unwrap();
    assert_eq!((part.positives().count(), part.negatives().count()), (2, 0));
    assert!(e.can_explode());

    // At the cursor.
    e.paste(&clip, true);
    assert!(e.carrying());
    click(&mut e, DVec3::new(0.0, 300.0, 18.0));
    assert!(!e.carrying());
    let (_, part, _) = e.document().resolve_part(&EntityPath::root(b)).unwrap();
    assert_eq!(part.positives().count(), 3);
}

#[test]
fn bodies_pasted_out_of_an_open_part_make_a_new_part() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(200.0, 50.0, 0.0), "18");
    e.enter(a);
    let blank = e.document().resolve_part(&EntityPath::root(a)).unwrap().1.bodies[0].id;
    e.select(vec![blank]);
    let clip = e.copy().unwrap();
    e.exit();
    assert!(e.can_paste(&clip));
    let parts = e.document().components().count();
    e.paste_in_place(&clip, true);
    assert_eq!(e.document().components().count(), parts + 1);
    let pasted = e.selection()[0];
    assert_ne!(pasted, a);
    assert_eq!(bounds_of(&e, &EntityPath::root(pasted)), bounds_of(&e, &EntityPath::root(a)));

    // At the cursor, from the low corner.
    e.paste(&clip, true);
    assert!(e.carrying());
    click(&mut e, DVec3::new(300.0, 300.0, 0.0));
    let (lo, _) = bounds_of(&e, &EntityPath::root(e.selection()[0]));
    assert!(close(lo, DVec3::new(300.0, 300.0, 0.0)), "{lo:?}");
}

#[test]
fn paste_at_the_cursor_is_carried_from_the_low_corner_then_put_down_in_one_step() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::new(-400.0, -300.0, 0.0), DVec3::new(-300.0, -250.0, 0.0), "20");
    let clip = e.copy().unwrap();
    let before = e.history().revision();

    // Carried by Move, already pasted (selected), until the click.
    e.paste(&clip, true);
    assert!(e.carrying());
    assert_eq!(e.tool_kind(), ToolKind::Move);
    let pasted = e.selection()[0];
    assert_ne!(pasted, a);
    assert!(e.status().contains("collé"), "{}", e.status());
    // Copying and other commands wait for it to be put down.
    assert!(!e.can_copy());
    click(&mut e, DVec3::new(300.0, 300.0, 0.0));
    assert!(!e.carrying());
    assert_eq!(e.tool_kind(), ToolKind::Select, "back to the tool before");
    let at = e.selection()[0];
    let (lo, hi) = bounds_of(&e, &EntityPath::root(at));
    assert!(close(lo, DVec3::new(300.0, 300.0, 0.0)) && close(hi, DVec3::new(400.0, 350.0, 20.0)), "{lo:?} {hi:?}");
    assert_eq!(e.document().components().count(), 1, "the same part");
    // One step: undone, the document is as before the paste.
    e.undo();
    assert_eq!(e.history().revision(), before);
    assert_eq!(e.document().root_assembly().occurrences().len(), 1);
}

#[test]
fn a_paste_carried_is_given_up_by_escape_undo_or_another_tool() {
    let mut e = top_editor(Document::new());
    draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 50.0, 0.0), "20");
    let clip = e.copy().unwrap();
    let before = e.history().revision();
    let count = |e: &Editor| e.document().root_assembly().occurrences().len();

    e.paste(&clip, true);
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(300.0, 0.0, 0.0)) });
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    assert!(!e.carrying() && count(&e) == 1 && e.history().revision() == before);
    assert_eq!(e.tool_kind(), ToolKind::Select);
    assert!(e.selection().is_empty());

    e.paste(&clip, true);
    e.undo();
    assert!(!e.carrying() && count(&e) == 1);
    // Undo gave up the paste only: the box is still there to undo.
    assert!(e.history().can_undo());

    e.set_tool(ToolKind::Erase);
    e.paste(&clip, true);
    e.set_tool(ToolKind::Rotate);
    assert!(!e.carrying() && count(&e) == 1);
    assert_eq!(e.tool_kind(), ToolKind::Rotate);
    // Put down with the Erase tool on before: back to it.
    e.set_tool(ToolKind::Erase);
    e.paste(&clip, true);
    click(&mut e, DVec3::new(0.0, 300.0, 0.0));
    assert_eq!(e.tool_kind(), ToolKind::Erase);
    assert_eq!(count(&e), 2);
}

#[test]
fn a_typed_distance_puts_the_paste_down() {
    let mut e = top_editor(Document::new());
    draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 50.0, 0.0), "20");
    let clip = e.copy().unwrap();
    e.paste(&clip, true);
    e.input(ToolInput::Key { key: Key::Right, mods: Modifiers::default() });
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(150.0, 0.0, 0.0)) });
    e.input(ToolInput::Vcb("500".into()));
    assert!(!e.carrying());
    let (lo, _) = bounds_of(&e, &EntityPath::root(e.selection()[0]));
    assert!(close(lo, DVec3::new(500.0, 0.0, 0.0)), "{lo:?}");
}

#[test]
fn move_rotate_and_the_tape_write_their_measure_on_lines_seen_through_faces() {
    let mut e = top_editor(Document::new());
    draw_box(&mut e, DVec3::new(-400.0, -300.0, 0.0), DVec3::new(-300.0, -250.0, 0.0), "20");

    // Move: the line from the start, x-rayed, its length at its middle.
    e.set_tool(ToolKind::Move);
    click(&mut e, DVec3::new(-400.0, -300.0, 0.0));
    e.input(ToolInput::Key { key: Key::Right, mods: Modifiers::default() });
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(-150.0, -300.0, 0.0)) });
    let p = e.preview();
    assert_eq!(x_rays(&p).len(), 1);
    // An arrow: its head at the destination, over the scene.
    let heads: Vec<_> = p.drawing.lines.iter().filter(|l| l.pen.layer == crate::draw::Layer::Over).collect();
    assert_eq!(heads.len(), 2);
    // (The start is on the box's top: the line runs at z = 20.)
    assert!(heads.iter().all(|l| close(l.b.with_z(0.0), DVec3::new(-150.0, -300.0, 0.0)) && l.a.x < -150.0), "{heads:?}");
    assert_eq!(p.drawing.lengths.len(), 1);
    assert!(p.drawing.lengths[0].text.starts_with("250"), "{}", p.drawing.lengths[0].text);
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });

    // Rotate: both radii dimensions' lines (dashed in the scene, translucent where hidden), the
    // angle between them.
    e.set_tool(ToolKind::Rotate);
    click(&mut e, DVec3::new(0.0, 0.0, 0.0));
    click(&mut e, DVec3::new(400.0, 0.0, 0.0));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(0.0, 400.0, 0.0)) });
    let p = e.preview();
    let radii = x_rays(&p);
    assert_eq!(radii.len(), 2);
    for end in [DVec3::new(400.0, 0.0, 0.0), DVec3::new(0.0, 400.0, 0.0)] {
        assert!(radii.iter().any(|l| close(l.b, end) && l.pen == crate::draw::Pen::dimension(l.pen.color)), "{end}");
    }
    assert_eq!(p.drawing.lengths.len(), 1);
    assert_eq!(p.drawing.lengths[0].text, "90°");
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });

    // The tape: its measure x-rayed too, its length on it.
    e.set_tool(ToolKind::Measure);
    click(&mut e, DVec3::new(-600.0, 400.0, 0.0));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(-200.0, 400.0, 0.0)) });
    let p = e.preview();
    assert_eq!(x_rays(&p).len(), 1);
    // A double arrow: a head at the cursor and one at the start.
    let over = |l: &&crate::draw::Line| l.pen.layer == crate::draw::Layer::Over;
    for end in [DVec3::new(-200.0, 400.0, 0.0), DVec3::new(-600.0, 400.0, 0.0)] {
        assert_eq!(p.drawing.lines.iter().filter(over).filter(|l| close(l.b, end)).count(), 2, "{end}");
    }
    let lengths = &p.drawing.lengths;
    assert!(lengths.iter().any(|l| l.text.starts_with("400")), "{lengths:?}");
}

#[test]
fn standard_views_are_in_the_active_frame() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "50");
    let tilt = glam::DAffine3::from_rotation_x(30f64.to_radians()) * glam::DAffine3::from_rotation_z(20f64.to_radians());
    let context = e.document().context_of(e.context()).unwrap();
    e.apply(Operation::Transform { context, entities: vec![a], transform: tilt }).unwrap();
    // At the model's level: the world's axes.
    e.standard_view(StandardView::Front);
    assert!(close(e.camera.forward(), DVec3::Y) && close(e.camera.up(), DVec3::Z));
    // Its part open: its axes, tilted.
    e.enter(a);
    assert_eq!(e.open_part(), Some(a));
    e.standard_view(StandardView::Front);
    assert!(close(e.camera.forward(), tilt.matrix3.y_axis) && close(e.camera.up(), tilt.matrix3.z_axis));
    e.standard_view(StandardView::Top);
    assert!(e.camera.forward().dot(-tilt.matrix3.z_axis) > 0.999 && e.camera.up().dot(tilt.matrix3.y_axis) > 0.999);
    // The orbit keeps its Z up, the part closed as well, until the next standard view.
    e.nav_begin(DVec2::splat(500.0));
    e.orbit(DVec2::new(10.0, 0.0));
    assert!(e.camera.right().dot(tilt.matrix3.z_axis).abs() < 1e-12);
    e.exit();
    e.orbit(DVec2::new(10.0, 0.0));
    assert!(e.camera.right().dot(tilt.matrix3.z_axis).abs() < 1e-12);
    // Back to the world's axes.
    e.standard_view(StandardView::Top);
    assert!(e.camera.frame == glam::DMat3::IDENTITY && e.camera.up().y > 0.99);
}

/// Parts seen as before docs/model/bodies.md (a blank and its machinings), for these tests.
pub(crate) trait PartExt {
    /// The first positive body's solid.
    fn blank(&self) -> std::sync::Arc<skraft_model::kernel::Solid>;
    /// The negative bodies.
    fn machinings(&self) -> Vec<skraft_model::Body>;
}

impl PartExt for skraft_model::Part {
    fn blank(&self) -> std::sync::Arc<skraft_model::kernel::Solid> {
        self.solid_of(self.positives().next().unwrap()).clone()
    }

    fn machinings(&self) -> Vec<skraft_model::Body> {
        self.negatives().cloned().collect()
    }
}

#[test]
fn positive_bodies_are_drawn_picked_erased_and_their_profile_edited_in_the_open_part() {
    use skraft_model::Sign;
    let mut e = top_editor(Document::new());
    let board = draw_box(&mut e, DVec3::ZERO, DVec3::new(400.0, 200.0, 0.0), "18");
    let path = EntityPath::root(board);
    e.enter(board);
    let blank = e.document().resolve_part(&path).unwrap().1.bodies[0].id;
    // « Ajouter »: a block drawn on the board's top joins it (docs/model/bodies.md).
    assert_eq!(e.body_sign(), Sign::Negative);
    e.set_body_sign(Sign::Positive);
    e.set_tool(ToolKind::Box);
    click(&mut e, DVec3::new(300.0, 50.0, 18.0));
    click(&mut e, DVec3::new(380.0, 150.0, 18.0));
    e.input(ToolInput::Vcb("30".into()));
    let p = e.document().resolve_part(&path).unwrap().1;
    assert_eq!((p.positives().count(), p.negatives().count()), (2, 0), "{}", e.status());
    assert_eq!(p.pieces(), 1);
    assert!((p.shape().bounds().unwrap().1.z - 48.0).abs() < 1e-6);
    let block = p.positives().nth(1).unwrap().id;

    // Picked as a body, the nearest positive one under the cursor.
    e.set_tool(ToolKind::Select);
    click(&mut e, DVec3::new(340.0, 100.0, 48.0));
    assert_eq!(e.selection(), &[block]);
    click(&mut e, DVec3::new(100.0, 100.0, 18.0));
    assert_eq!(e.selection(), &[blank]);

    // A double-click on a body edits its profile.
    e.input(ToolInput::DoubleClick { px: px(&e, DVec3::new(340.0, 100.0, 48.0)), mods: Modifiers::default() });
    assert_eq!(e.tool_kind(), ToolKind::Profile);
    assert_eq!(e.open_part(), Some(board));

    // The body open, the deepest level (docs/model/bodies.md#edit-levels): its box is the context's; Esc
    // leaves it for the part, the body selected; Esc again, nothing selected, closes the part.
    assert_eq!(e.profile_body(), Some(block));
    // The other bodies faded, the one open not.
    let faded = |e: &Editor| {
        let v = e.view();
        let blanks: Vec<bool> = v.batches.iter().flat_map(|(_, p)| p).filter(|p| p.look == Look::Blank).map(|p| p.faded).collect();
        (blanks.len(), blanks.iter().filter(|f| **f).count())
    };
    assert_eq!(faded(&e), (2, 1));
    let esc = ToolInput::Key { key: Key::Escape, mods: Modifiers::default() };
    e.input(esc.clone());
    assert_eq!(faded(&e), (2, 0));
    assert_eq!(e.tool_kind(), ToolKind::Select);
    assert_eq!(e.open_part(), Some(board));
    assert_eq!(e.selection(), &[block]);
    // A click off the body's box leaves it too, a click inside does not.
    e.input(ToolInput::DoubleClick { px: px(&e, DVec3::new(340.0, 100.0, 48.0)), mods: Modifiers::default() });
    assert_eq!(e.profile_body(), Some(block));
    click(&mut e, DVec3::new(100.0, 250.0, 18.0));
    assert_eq!((e.tool_kind(), e.open_part()), (ToolKind::Select, Some(board)));
    assert_eq!(e.selection(), &[block]);
    // Another tool chosen leaves it as well.
    e.input(ToolInput::DoubleClick { px: px(&e, DVec3::new(340.0, 100.0, 48.0)), mods: Modifiers::default() });
    e.set_tool(ToolKind::Move);
    assert_eq!(e.profile_body(), None);
    assert_eq!(e.open_part(), Some(board));

    // The block erased; the blank, last positive body, too: the part is empty (docs/model/bodies.md « Empty
    // parts »).
    e.set_tool(ToolKind::Select);
    e.select(vec![block]);
    e.input(ToolInput::Key { key: Key::Delete, mods: Modifiers::default() });
    assert_eq!(e.document().resolve_part(&path).unwrap().1.bodies.len(), 1);
    e.select(vec![blank]);
    e.input(ToolInput::Key { key: Key::Delete, mods: Modifiers::default() });
    let p = e.document().resolve_part(&path).unwrap().1.clone();
    assert!(p.bodies.is_empty() && p.shape().is_empty() && p.pieces() == 0);

    // Reopened, a shape drawn takes material away again.
    e.exit();
    e.enter(board);
    assert_eq!(e.body_sign(), Sign::Negative);
}

#[test]
fn stretch_tool_stretches_the_bodies_selected_alone() {
    use glam::DAffine3;
    use skraft_model::{Scope, Sign};
    let mut e = top_editor(Document::new());
    let board = draw_box(&mut e, DVec3::ZERO, DVec3::new(400.0, 200.0, 0.0), "18");
    let path = EntityPath::root(board);
    let block = Primitive::Box { origin: DVec3::new(300.0, 50.0, 18.0), size: DVec3::new(80.0, 100.0, 30.0) };
    let block = e
        .apply(Operation::AddBody {
            target: path.clone(),
            primitive: block,
            placement: DAffine3::IDENTITY,
            sign: Sign::Positive,
            scope: Scope::All,
        })
        .unwrap()
        .unwrap()
        .bodies[0];
    let board_body = e.document().resolve_part(&path).unwrap().1.positives().next().unwrap().clone();
    e.enter(board);
    e.select(vec![block]);
    e.set_tool(ToolKind::Stretch);
    // The block's grip on +X, pulled: it alone drawn stretched.
    click(&mut e, DVec3::new(380.0, 100.0, 48.0));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(600.0, 100.0, 48.0)) });
    assert!(e.view().batches.iter().any(|(k, _)| *k == crate::scene::PREVIEW_KEY), "{}", e.status());
    // Typed: 120 long overall.
    e.input(ToolInput::Vcb("120".into()));
    let p = e.document().resolve_part(&path).unwrap().1;
    let b = p.body(block).unwrap();
    assert_eq!(skraft_model::kernel::profile::bounds(&p.profile_of(b).unwrap().profile).unwrap().1, DVec2::new(120.0, 100.0));
    assert_eq!(p.positives().next().unwrap(), &board_body, "the board untouched");
    assert!((p.shape().bounds().unwrap().1.x - 420.0).abs() < 1e-6, "{}", e.status());
    assert_eq!(e.open_part(), Some(board));
}

#[test]
fn a_click_in_the_outliner_selects_or_deselects() {
    let mut e = top_editor(Document::new());
    two_boxes(&mut e);
    let ids: Vec<Id> = e.document().root_assembly().occurrences().iter().map(|o| o.id).collect();
    let (a, b) = (EntityPath::root(ids[0]), EntityPath::root(ids[1]));
    e.toggle_path(&a, false);
    assert_eq!(e.selection(), &[ids[0]]);
    e.toggle_path(&b, false);
    assert_eq!(e.selection(), &[ids[1]], "another one selected alone");
    e.toggle_path(&b, false);
    assert!(e.selection().is_empty(), "selected, deselected");
    // With Shift: added, then taken out alone.
    e.toggle_path(&a, true);
    e.toggle_path(&b, true);
    assert_eq!(e.selection(), &[ids[0], ids[1]]);
    e.toggle_path(&a, true);
    assert_eq!(e.selection(), &[ids[1]]);
}

#[test]
fn copy_along_a_line_by_a_click_then_a_count_and_a_distance() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "50");
    let occurrences = |e: &Editor| e.document().root_assembly().occurrences().len();
    let xs = |e: &Editor| {
        let mut xs: Vec<f64> =
            e.document().root_assembly().occurrences().iter().map(|o| bounds_of(e, &EntityPath::root(o.id)).0.x).collect();
        xs.sort_by(f64::total_cmp);
        xs
    };
    // The selection taken at once: off the geometry, the cursor runs along the axis nearest it
    // on screen, from the box's centre; edge to edge, 500 from the source's back.
    e.set_tool(ToolKind::CopyLine);
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(500.0, 60.0, 0.0)) });
    let v = e.view();
    assert!(v.highlights.iter().any(|h| h.mark == Mark::Ghost), "the copy drawn as a ghost");
    assert!(boxes_in(&v, ThemeColor::Selected).is_empty(), "the tool draws its own boxes");
    // A box fills its box: none drawn.
    assert!(v.drawing.boxes.is_empty(), "a box part's box adds nothing");
    assert_eq!(e.vcb().map(|v| v.1), Some("500".to_owned()));
    e.input(ToolInput::Click { px: px(&e, DVec3::new(500.0, 60.0, 0.0)), mods: Modifiers::default() });
    assert_eq!(occurrences(&e), 2);
    // (To the pixel's precision.)
    assert!(xs(&e).iter().zip([0.0, 400.0]).all(|(x, w)| (x - w).abs() < 1e-3), "{:?}", xs(&e));
    assert!(e.selection().is_empty(), "the copies let go");

    // Typed again: 4 copies over the same way, then 1 000 edge to edge; one undo step.
    let steps = e.history().journal().count();
    e.input(ToolInput::Vcb("/4".into()));
    assert!(xs(&e).iter().zip([0.0, 100.0, 200.0, 300.0, 400.0]).all(|(x, w)| (x - w).abs() < 1e-3), "{:?}", xs(&e));
    e.input(ToolInput::Vcb("1000".into()));
    assert!(xs(&e).iter().zip([0.0, 225.0, 450.0, 675.0, 900.0]).all(|(x, w)| (x - w).abs() < 1e-6), "{:?}", xs(&e));
    // Less than the copies take: refused.
    e.input(ToolInput::Vcb("300".into()));
    assert_eq!(occurrences(&e), 5);
    assert_eq!(e.history().journal().count(), steps, "in place of the copy");
    e.undo();
    assert_eq!(occurrences(&e), 1);

    // Nothing selected: a click picks; the mirror makes one copy, mirrored.
    e.select(vec![]);
    e.set_tool(ToolKind::CopyLine);
    click(&mut e, DVec3::new(50.0, 50.0, 50.0));
    assert_eq!(e.selection(), &[a]);
    e.input(ToolInput::Key { key: Key::Copy, mods: Modifiers::default() });
    // ← locks green: the cursor off it still goes along it.
    e.input(ToolInput::Key { key: Key::Left, mods: Modifiers::default() });
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(500.0, 600.0, 0.0)) });
    let badge = |e: &Editor| e.overlay_points().iter().any(|p| p.shape == crate::tools::PointShape::Mirror);
    assert!(badge(&e), "the mirror's badge past the copy");
    e.input(ToolInput::Vcb("2x".into()));
    assert!(!badge(&e), "not for several copies");
    e.input(ToolInput::Vcb("/1".into()));
    e.input(ToolInput::Vcb("200".into()));
    let copy = e.document().root_assembly().occurrences().iter().find(|o| o.id != a).unwrap().clone();
    assert!(copy.transform.matrix3.determinant() < 0.0, "mirrored");
    let (lo, hi) = bounds_of(&e, &EntityPath::root(copy.id));
    assert!(close(lo, DVec3::new(0.0, 100.0, 0.0)) && close(hi, DVec3::new(100.0, 200.0, 50.0)), "{lo} {hi}");
}

#[test]
fn shift_plus_minus_step_the_copies_count() {
    let key = |e: &mut Editor, key: Key| e.input(ToolInput::Key { key, mods: Modifiers::default() });
    let occurrences = |e: &Editor| e.document().root_assembly().occurrences().len();
    let ghosts = |e: &Editor| e.view().highlights.iter().filter(|h| h.mark == Mark::Ghost).count();

    // Along a line: while placing only; never below one copy, `/` kept.
    let mut e = top_editor(Document::new());
    draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "50");
    e.set_tool(ToolKind::CopyLine);
    let at = px(&e, DVec3::new(500.0, 60.0, 0.0));
    e.input(ToolInput::Hover { px: at });
    assert!(e.takes_counts());
    key(&mut e, Key::More);
    key(&mut e, Key::More);
    assert_eq!(ghosts(&e), 3);
    assert_eq!(e.vcb().map(|v| v.1), Some("500".to_owned()), "three copies over the same way");
    for _ in 0..5 {
        key(&mut e, Key::Fewer);
    }
    assert_eq!(ghosts(&e), 1);
    key(&mut e, Key::More);
    e.input(ToolInput::Click { px: at, mods: Modifiers::default() });
    assert_eq!(occurrences(&e), 3);
    assert!(!e.takes_counts(), "the copies made: typed values redo them");

    // Along a grid: both sides at once.
    let mut e = top_editor(Document::new());
    draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "50");
    e.set_tool(ToolKind::CopyGrid);
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(500.0, 400.0, 0.0)) });
    assert_eq!(ghosts(&e), 3);
    key(&mut e, Key::More);
    assert_eq!(ghosts(&e), 8);
    key(&mut e, Key::Fewer);
    key(&mut e, Key::Fewer);
    assert_eq!(ghosts(&e), 3);

    // Distribute: at the second point only; the pieces in all.
    let mut e = top_editor(Document::new());
    e.set_tool(ToolKind::Box);
    click(&mut e, DVec3::ZERO);
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(100.0, 100.0, 0.0)) });
    e.input(ToolInput::Vcb("18;300".into()));
    e.input(ToolInput::Vcb("20".into()));
    let shelf = drawn(&e);
    e.set_tool(ToolKind::Select);
    e.select(vec![shelf]);
    e.set_tool(ToolKind::Distribute);
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(100.0, 150.0, 0.0)) });
    assert!(!e.takes_counts());
    click(&mut e, DVec3::new(100.0, 150.0, 0.0));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(700.0, 160.0, 0.0)) });
    assert!(e.takes_counts());
    key(&mut e, Key::More);
    key(&mut e, Key::More);
    e.input(ToolInput::Click { px: px(&e, DVec3::new(700.0, 160.0, 0.0)), mods: Modifiers::default() });
    assert_eq!(occurrences(&e), 3);
}

#[test]
fn copy_along_a_line_takes_the_bodies_of_the_open_part() {
    use glam::DAffine3;
    use skraft_model::Scope;
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let board = draw_box(&mut e, DVec3::ZERO, DVec3::new(400.0, 200.0, 0.0), "18");
    let drill = e
        .apply(Operation::AddPrimitive {
            context: root,
            primitive: Primitive::Cylinder { origin: DVec3::new(50.0, 100.0, -5.0), radius: 4.0, height: 30.0 },
            placement: DAffine3::IDENTITY,
            material: None,
        })
        .unwrap()
        .unwrap()
        .created[0];
    let target = EntityPath::root(board);
    e.apply(Operation::boolean(target.clone(), EntityPath::root(drill), Boolean::Difference, Scope::All)).unwrap();
    let hole = e.document().resolve_part(&target).unwrap().1.machinings()[0].id;
    e.enter(board);
    e.select(vec![hole]);
    e.set_tool(ToolKind::CopyLine);
    // On the board's face, beside the hole: along X, the way leaving the hole's box.
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(300.0, 100.0, 18.0)) });
    e.input(ToolInput::Vcb("100".into()));
    let p = e.document().resolve_part(&target).unwrap().1.clone();
    let holes: Vec<f64> = p.machinings().iter().map(|m| m.placement.translation.x).collect();
    // Edge to edge 100 from the hole's back (x 46): the copy's centre at 142.
    assert_eq!(holes.len(), 2, "{holes:?}");
    assert!((holes[1] - 142.0).abs() < 1e-6, "{holes:?}");
}

#[test]
fn distribute_between_two_points_then_spacings_and_a_double_click() {
    let mut e = top_editor(Document::new());
    // Typed: a corner 18 from the first snaps to it on screen.
    e.set_tool(ToolKind::Box);
    click(&mut e, DVec3::ZERO);
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(100.0, 100.0, 0.0)) });
    e.input(ToolInput::Vcb("18;300".into()));
    e.input(ToolInput::Vcb("20".into()));
    let shelf = drawn(&e);
    e.set_tool(ToolKind::Select);
    e.select(vec![shelf]);
    let los = |e: &Editor| {
        let mut xs: Vec<f64> =
            e.document().root_assembly().occurrences().iter().map(|o| bounds_of(e, &EntityPath::root(o.id)).0.x).collect();
        xs.sort_by(f64::total_cmp);
        xs
    };
    let near = |a: &[f64], b: &[f64]| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3);
    e.set_tool(ToolKind::Distribute);
    click(&mut e, DVec3::new(100.0, 150.0, 0.0));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(700.0, 160.0, 0.0)) });
    // Drawn where it goes only, as a ghost.
    let shown = |e: &Editor| e.view().batches.iter().map(|(_, ps)| ps.len()).sum::<usize>();
    assert_eq!(shown(&e), 0);
    assert!(e.view().highlights.iter().any(|h| h.mark == Mark::Ghost));
    // Three pieces, even gaps between the parts: (600 − 54) / 4.
    e.input(ToolInput::Vcb("3x".into()));
    e.input(ToolInput::Click { px: px(&e, DVec3::new(700.0, 160.0, 0.0)), mods: Modifiers::default() });
    assert!(near(&los(&e), &[236.5, 391.0, 545.5]), "{:?}", los(&e));
    assert!(e.document().root_assembly().occurrences().iter().any(|o| o.id == shelf), "the source moved, not copied");
    assert!(e.selection().is_empty());

    // The first gap 30, then the rest spread: in place of the first, one undo step.
    e.input(ToolInput::Vcb("30;".into()));
    let x = los(&e);
    assert_eq!(x.len(), 3);
    assert!((x[0] - 130.0).abs() < 1e-3, "{x:?}");
    e.undo();
    assert!(near(&los(&e), &[0.0]), "{:?}", los(&e));

    // A double-click on the shelf: the last distribute again.
    let at = px(&e, DVec3::new(9.0, 150.0, 20.0));
    e.input(ToolInput::Hover { px: at });
    e.input(ToolInput::Click { px: at, mods: Modifiers::default() });
    e.input(ToolInput::DoubleClick { px: at, mods: Modifiers::default() });
    let x = los(&e);
    assert_eq!(x.len(), 3, "{x:?}");
    assert!((x[0] - 130.0).abs() < 1e-3, "{x:?}");
}

#[test]
fn copy_along_a_grid_by_a_click_then_counts_and_sides() {
    let mut e = top_editor(Document::new());
    draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "50");
    let occurrences = |e: &Editor| e.document().root_assembly().occurrences().len();
    let lows = |e: &Editor| -> Vec<(f64, f64)> {
        let mut v: Vec<(f64, f64)> = e
            .document()
            .root_assembly()
            .occurrences()
            .iter()
            .map(|o| bounds_of(e, &EntityPath::root(o.id)).0)
            // To the pixel's precision.
            .map(|p| ((p.x * 10.0).round() / 10.0, (p.y * 10.0).round() / 10.0))
            .collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v
    };
    e.set_tool(ToolKind::CopyGrid);
    let at = px(&e, DVec3::new(500.0, 400.0, 0.0));
    e.input(ToolInput::Hover { px: at });
    // Edge to edge, 500 × 400 from the source's corner: three copies.
    // The cursor's point, to the pixel: maybe marked rounded.
    assert_eq!(e.vcb().map(|v| v.1.replace("~ ", "")), Some("500; 400".to_owned()));
    assert_eq!(e.view().highlights.iter().filter(|h| h.mark == Mark::Ghost).count(), 3);
    e.input(ToolInput::Click { px: at, mods: Modifiers::default() });
    assert_eq!(lows(&e), vec![(0.0, 0.0), (0.0, 300.0), (400.0, 0.0), (400.0, 300.0)]);
    assert!(e.selection().is_empty());

    // Two at the step along red, two over the way along green: 3 × 3.
    e.input(ToolInput::Vcb("2x;/2".into()));
    assert_eq!(occurrences(&e), 9);
    assert!(lows(&e).contains(&(800.0, 150.0)), "{:?}", lows(&e));
    // Then the sides: 1 000 × 600.
    e.input(ToolInput::Vcb("1000;600".into()));
    assert_eq!(occurrences(&e), 9);
    assert_eq!(lows(&e).last(), Some(&(1800.0, 500.0)), "{:?}", lows(&e));
    e.undo();
    assert_eq!(occurrences(&e), 1);
}

/// Draws a box and stays in the Box tool: nothing selected, the quick tools' card offered on it.
fn box_then_quick(e: &mut Editor) -> Id {
    e.set_tool(ToolKind::Box);
    click(e, DVec3::ZERO);
    click(e, DVec3::new(100.0, 100.0, 0.0));
    e.input(ToolInput::Vcb("50".into()));
    assert!(e.selection().is_empty());
    drawn(e)
}

#[test]
fn the_quick_tools_card_shows_on_what_was_drawn_until_the_next_shape() {
    let mut e = top_editor(Document::new());
    let a = box_then_quick(&mut e);
    // At the centre of the box, on screen.
    let at = e.quick_card().expect("the card after a creation");
    assert!((at - px(&e, DVec3::new(50.0, 50.0, 25.0))).length() < 1e-6, "{at}");
    // The view moved: the card follows.
    e.zoom_extents();
    let (q, p) = (e.quick_card().unwrap(), px(&e, DVec3::new(50.0, 50.0, 25.0)));
    // (The box read from its mesh: to a float's precision.)
    assert!((q - p).length() < 1e-3, "{q} {p}");
    // The next shape started: gone, and not back once it is given up.
    click(&mut e, DVec3::new(0.0, 500.0, 0.0));
    assert!(e.quick_card().is_none());
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    assert!(e.quick_card().is_none());

    // Undone, Alt, or another tool: gone.
    box_then_quick(&mut e);
    assert!(e.quick_card().is_some());
    e.undo();
    assert!(e.quick_card().is_none());
    box_then_quick(&mut e);
    e.input(ToolInput::Key { key: Key::Alt, mods: Modifiers::default() });
    assert!(e.quick_card().is_none());
    box_then_quick(&mut e);
    e.set_tool(ToolKind::Select);
    assert!(e.quick_card().is_none());

    // A tool started from it takes what was drawn; given up (Esc), the card and the selection
    // are back as they were.
    let b = box_then_quick(&mut e);
    assert_ne!(a, b);
    e.start_quick_tool(ToolKind::CopyLine);
    assert_eq!(e.selection(), &[b]);
    assert!(e.quick_card().is_none(), "not while the tool runs");
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(500.0, 60.0, 0.0)) });
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    assert_eq!(e.tool_kind(), ToolKind::Box);
    assert!(e.quick_card().is_some(), "offered again: nothing was done");
    assert!(e.selection().is_empty());
    // Done: not offered again.
    e.start_quick_tool(ToolKind::CopyLine);
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(500.0, 60.0, 0.0)) });
    e.input(ToolInput::Click { px: px(&e, DVec3::new(500.0, 60.0, 0.0)), mods: Modifiers::default() });
    assert_eq!(e.tool_kind(), ToolKind::Box);
    assert!(e.quick_card().is_none());
}

#[test]
fn the_quick_tools_card_shows_on_a_body_drawn_in_the_open_part() {
    let mut e = top_editor(Document::new());
    let board = draw_box(&mut e, DVec3::ZERO, DVec3::new(400.0, 200.0, 0.0), "18");
    e.enter(board);
    e.select(vec![]);
    e.set_tool(ToolKind::Box);
    click(&mut e, DVec3::new(100.0, 50.0, 18.0));
    click(&mut e, DVec3::new(200.0, 150.0, 18.0));
    e.input(ToolInput::Vcb("10".into()));
    let body = drawn(&e);
    let at = e.quick_card().expect("the card on the body");
    assert!((at - px(&e, DVec3::new(150.0, 100.0, 23.0))).length() < 1e-6, "{at}");
    e.start_quick_tool(ToolKind::CopyLine);
    assert_eq!(e.selection(), &[body]);
}

#[test]
fn a_quick_tool_hands_back_to_the_creation_tool_and_takes_what_is_typed_next() {
    let mut e = top_editor(Document::new());
    box_then_quick(&mut e);
    let occurrences = |e: &Editor| e.document().root_assembly().occurrences().len();
    e.start_quick_tool(ToolKind::CopyLine);
    assert!(e.quick_tool_running());
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(500.0, 60.0, 0.0)) });
    e.input(ToolInput::Click { px: px(&e, DVec3::new(500.0, 60.0, 0.0)), mods: Modifiers::default() });
    // The copy made: back to the Box tool, the copy tool aside.
    assert_eq!(occurrences(&e), 2);
    assert_eq!(e.tool_kind(), ToolKind::Box);
    assert!(!e.quick_tool_running());
    assert_eq!(e.quick_tool_aside(), Some(ToolKind::CopyLine));
    assert_eq!(e.vcb().map(|v| v.1), Some("500".to_owned()), "the copy's measure shown");
    // Typed before the next shape: the copies redone, one undo step (not the Box's offset).
    let steps = e.history().journal().count();
    e.input(ToolInput::Vcb("/4".into()));
    assert_eq!(occurrences(&e), 5);
    assert_eq!(e.history().journal().count(), steps);
    assert_eq!(e.tool_kind(), ToolKind::Box);
    // The next shape started: the tool aside let go.
    click(&mut e, DVec3::new(0.0, 500.0, 0.0));
    assert_eq!(e.quick_tool_aside(), None);
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    assert_eq!(e.tool_kind(), ToolKind::Box);
}

#[test]
fn escape_gives_a_quick_tool_up_and_a_tool_chosen_drops_the_return() {
    let mut e = top_editor(Document::new());
    box_then_quick(&mut e);
    let steps = e.history().journal().count();
    e.start_quick_tool(ToolKind::CopyLine);
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(500.0, 60.0, 0.0)) });
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    assert_eq!(e.tool_kind(), ToolKind::Box);
    assert_eq!(e.quick_tool_aside(), None, "nothing done, nothing aside");
    assert_eq!(e.history().journal().count(), steps);

    // Another tool chosen meanwhile: no return.
    e.select(vec![drawn(&e)]);
    e.start_quick_tool(ToolKind::CopyLine);
    e.set_tool(ToolKind::CopyGrid);
    assert!(!e.quick_tool_running());

    // Esc at the tool aside lets it go, the creation tool stays.
    e.set_tool(ToolKind::Box);
    e.select(vec![drawn(&e)]);
    e.start_quick_tool(ToolKind::CopyLine);
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(500.0, 60.0, 0.0)) });
    e.input(ToolInput::Click { px: px(&e, DVec3::new(500.0, 60.0, 0.0)), mods: Modifiers::default() });
    assert_eq!(e.quick_tool_aside(), Some(ToolKind::CopyLine));
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    assert_eq!(e.quick_tool_aside(), None);
    assert_eq!(e.tool_kind(), ToolKind::Box);
}

#[test]
fn move_started_as_a_quick_tool_hands_back_after_its_copy_and_takes_a_count() {
    let mut e = top_editor(Document::new());
    box_then_quick(&mut e);
    let occurrences = |e: &Editor| e.document().root_assembly().occurrences().len();
    e.start_quick_tool(ToolKind::Move);
    e.input(ToolInput::Key { key: Key::Copy, mods: Modifiers::default() });
    click(&mut e, DVec3::new(100.0, 100.0, 0.0));
    assert_eq!(e.tool_kind(), ToolKind::Move, "the first click is not the action");
    click(&mut e, DVec3::new(200.0, 100.0, 0.0));
    assert_eq!(occurrences(&e), 2);
    assert_eq!(e.tool_kind(), ToolKind::Box);
    e.input(ToolInput::Vcb("3x".into()));
    assert_eq!(occurrences(&e), 4, "{}", e.status());
}

#[test]
fn distribute_and_paint_started_as_quick_tools_hand_back_at_their_action() {
    let mut e = top_editor(Document::new());
    let a = box_then_quick(&mut e);
    // Distribute: its first click is not the action, its second is; then a count retyped.
    e.start_quick_tool(ToolKind::Distribute);
    click(&mut e, DVec3::new(100.0, 150.0, 0.0));
    assert_eq!(e.tool_kind(), ToolKind::Distribute);
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(700.0, 160.0, 0.0)) });
    e.input(ToolInput::Vcb("3x".into()));
    e.input(ToolInput::Click { px: px(&e, DVec3::new(700.0, 160.0, 0.0)), mods: Modifiers::default() });
    assert_eq!(e.document().root_assembly().occurrences().len(), 3);
    assert_eq!(e.tool_kind(), ToolKind::Box);
    assert_eq!(e.quick_tool_aside(), Some(ToolKind::Distribute));
    e.input(ToolInput::Vcb("4x".into()));
    assert_eq!(e.document().root_assembly().occurrences().len(), 4, "{}", e.status());
    e.undo();

    // Paint: its first click paints, and hands back.
    e.apply(Operation::AddMaterial { name: "Chêne".into(), color: [180, 140, 90] }).unwrap();
    let oak = e.document().materials().next().unwrap().0;
    e.set_paint_material(Some(oak));
    e.set_tool(ToolKind::Box);
    e.select(vec![a]);
    e.start_quick_tool(ToolKind::Paint);
    click(&mut e, DVec3::new(50.0, 50.0, 50.0));
    assert_eq!(drawn_material(&e, a), Some(oak));
    assert_eq!(e.tool_kind(), ToolKind::Box);
}

/// Drags a shape tool's button from `a` to `b` (world points, seen from the top), the cursor
/// moving on the way as the viewport sends it.
fn drag_tool(e: &mut Editor, a: DVec3, b: DVec3) {
    let (pa, pb) = (px(e, a), px(e, b));
    let mods = Modifiers::default();
    e.input(ToolInput::Hover { px: pa });
    e.input(ToolInput::DragStart { px: pa, mods });
    e.input(ToolInput::Drag { px: (pa + pb) / 2.0 });
    e.input(ToolInput::Drag { px: pb });
    e.input(ToolInput::DragEnd { px: pb, mods });
}

#[test]
fn a_drag_is_click_move_click_in_the_shape_tools() {
    use skraft_i18n::t;
    let (copy_key, alt_key) = (crate::tools::copy_key(), crate::tools::alt_key());
    let mut e = top_editor(Document::new());

    // Box: the press is the first corner, the release the second; the height is next.
    e.set_tool(ToolKind::Box);
    drag_tool(&mut e, DVec3::ZERO, DVec3::new(300.0, 200.0, 0.0));
    assert_eq!(e.status(), t!("tool.shape.hint.height", copy_key = copy_key.clone()));
    e.input(ToolInput::Vcb("150".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(close(lo, DVec3::ZERO) && close(hi, DVec3::new(300.0, 200.0, 150.0)), "{lo} {hi}");

    // Released near the press (10 mm, 3 px: under 20 px): only the first click.
    drag_tool(&mut e, DVec3::new(-500.0, -500.0, 0.0), DVec3::new(-490.0, -500.0, 0.0));
    assert_eq!(e.status(), t!("tool.box.hint.base", copy_key = copy_key.clone()));
    // At a later step, a drag clicks where it is released.
    drag_tool(&mut e, DVec3::new(-300.0, -300.0, 0.0), DVec3::new(-400.0, -450.0, 0.0));
    assert_eq!(e.status(), t!("tool.shape.hint.height", copy_key = copy_key.clone()));
    e.input(ToolInput::Vcb("10".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(close(lo, DVec3::new(-500.0, -500.0, 0.0)) && close(hi, DVec3::new(-400.0, -450.0, 10.0)), "{lo} {hi}");

    // Esc during the drag: the release does nothing.
    let mods = Modifiers::default();
    let (pa, pb) = (px(&e, DVec3::new(500.0, 500.0, 0.0)), px(&e, DVec3::new(700.0, 700.0, 0.0)));
    e.input(ToolInput::DragStart { px: pa, mods });
    e.input(ToolInput::Drag { px: pb });
    e.input(ToolInput::Key { key: Key::Escape, mods });
    e.input(ToolInput::DragEnd { px: pb, mods });
    assert_eq!(e.status(), t!("tool.box.hint.first", copy_key = copy_key.clone(), alt_key = alt_key));

    // Cylinder: the centre, then the radius.
    e.set_tool(ToolKind::Cylinder);
    drag_tool(&mut e, DVec3::new(100.0, 600.0, 0.0), DVec3::new(250.0, 600.0, 0.0));
    e.input(ToolInput::Vcb("30".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!((lo.x - 250.0).abs() < 1e-3 && (hi.z - 30.0).abs() < 1e-6, "radius 150: {lo} {hi}");

    // Prism: the first two corners, then clicks.
    e.set_tool(ToolKind::Prism);
    drag_tool(&mut e, DVec3::new(-600.0, 300.0, 0.0), DVec3::new(-200.0, 300.0, 0.0));
    corners(&mut e, &[(-200.0, 500.0), (-600.0, 500.0), (-600.0, 300.0)]);
    assert!(e.status().contains("hauteur"), "{}", e.status());
    e.input(ToolInput::Vcb("18".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(close(lo, DVec3::new(-600.0, 300.0, 0.0)) && close(hi, DVec3::new(-200.0, 500.0, 18.0)), "{lo} {hi}");

    // Turn: the axis's two ends.
    e.set_tool(ToolKind::Turn);
    drag_tool(&mut e, DVec3::new(0.0, -900.0, 0.0), DVec3::new(0.0, -300.0, 0.0));
    assert!(e.status().contains("demi-profil"), "{}", e.status());
}

#[test]
fn a_drag_moves_from_the_press_to_the_release() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "50");
    e.select(vec![]);
    e.set_tool(ToolKind::Move);
    let mods = Modifiers::default();

    // Nothing taken, a drag is a selection rectangle (docs/tools/selection.md): from the void over half
    // the box, left to right, it encloses nothing.
    let (pa, pb) = (px(&e, DVec3::new(-600.0, -600.0, 0.0)), px(&e, DVec3::new(50.0, 50.0, 50.0)));
    e.input(ToolInput::DragStart { px: pa, mods });
    e.input(ToolInput::Drag { px: pb });
    e.input(ToolInput::DragEnd { px: pb, mods });
    assert!(e.selection().is_empty(), "{:?}", e.selection());
    assert!(close(bounds_of(&e, &EntityPath::root(a)).0, DVec3::ZERO));

    // The box picked, then pressed on it, dragged along red, released: moved.
    let (pa, pb) = (px(&e, DVec3::new(50.0, 50.0, 50.0)), px(&e, DVec3::new(350.0, 80.0, 50.0)));
    e.input(ToolInput::Hover { px: pa });
    e.input(ToolInput::Click { px: pa, mods });
    assert_eq!(e.selection(), &[a]);
    e.input(ToolInput::Hover { px: pa });
    e.input(ToolInput::DragStart { px: pa, mods });
    e.input(ToolInput::Key { key: Key::Right, mods });
    e.input(ToolInput::Drag { px: pb });
    e.input(ToolInput::DragEnd { px: pb, mods });
    let (lo, _) = bounds_of(&e, &EntityPath::root(a));
    assert!(close(lo, DVec3::new(300.0, 0.0, 0.0)), "{lo}");
}

#[test]
fn up_to_the_face_reaches_its_exact_plane() {
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    // A block floating above, its bottom off the middle of its bounds (where its display mesh,
    // `f32` about that middle, is some 1e-5 mm off it).
    let bottom = 496.59507189112446;
    let block = Primitive::Box { origin: DVec3::new(100.0, 100.0, bottom), size: DVec3::new(200.0, 200.0, 1500.0 - bottom) };
    e.apply(Operation::AddPrimitive { context: root, primitive: block, placement: glam::DAffine3::IDENTITY, material: None }).unwrap();

    // The copy modifier held for the height: up to the block's bottom, exactly (every shape
    // tool's height step pulls so; a base wider than the block, its corners in sight).
    e.set_tool(ToolKind::Box);
    click(&mut e, DVec3::new(50.0, 50.0, 0.0));
    click(&mut e, DVec3::new(350.0, 350.0, 0.0));
    e.set_copy(true);
    click(&mut e, DVec3::new(350.0, 350.0, 400.0));
    e.set_copy(false);
    let (lo, hi) = bounds_of(&e, &EntityPath::root(drawn(&e)));
    assert!(lo.z.abs() < 1e-9 && (hi.z - bottom).abs() < 1e-9, "{lo} {hi}");
}

#[test]
fn stretch_tool_makes_a_typed_length_however_close() {
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    // A board a hair short of 600.
    let board = Primitive::Box { origin: DVec3::ZERO, size: DVec3::new(600.0 - 1.4e-5, 300.0, 18.0) };
    e.apply(Operation::AddPrimitive { context: root, primitive: board, placement: glam::DAffine3::IDENTITY, material: None }).unwrap();
    let a = drawn(&e);
    e.select(vec![a]);
    e.set_tool(ToolKind::Stretch);
    let length = |e: &Editor| {
        let (lo, hi) = bounds_of(e, &EntityPath::root(a));
        hi.x - lo.x
    };
    // 600 typed: made, the 14 nm too.
    click(&mut e, DVec3::new(600.0, 150.0, 18.0));
    e.input(ToolInput::Vcb("600".into()));
    assert!((length(&e) - 600.0).abs() < 1e-9, "{} {}", length(&e), e.status());
    // A click without a move still does nothing.
    let revision = e.history().revision();
    click(&mut e, DVec3::new(600.0, 150.0, 18.0));
    click(&mut e, DVec3::new(600.0, 150.0, 18.0));
    assert_eq!(e.history().revision(), revision);

    // Short again; a corner of another box at 600 clicked: there too.
    e.undo();
    assert!((length(&e) - 600.0).abs() > 1e-6);
    let other = Primitive::Box { origin: DVec3::new(600.0, 400.0, 0.0), size: DVec3::new(100.0, 100.0, 18.0) };
    e.apply(Operation::AddPrimitive { context: root, primitive: other, placement: glam::DAffine3::IDENTITY, material: None }).unwrap();
    e.select(vec![a]);
    e.set_tool(ToolKind::Stretch);
    click(&mut e, DVec3::new(600.0, 150.0, 18.0));
    click(&mut e, DVec3::new(600.0, 400.0, 18.0));
    assert!((length(&e) - 600.0).abs() < 1e-9, "{} {}", length(&e), e.status());
}

/// The part of the occurrence `o` of the root.
fn part_of(e: &Editor, o: Id) -> Id {
    e.document().resolve(&EntityPath::root(o)).unwrap().component
}

#[test]
fn a_part_made_is_offered_its_name_by_a_toast() {
    let mut e = top_editor(Document::new());
    e.take_notices();
    let o = box_then_quick(&mut e);
    let part = part_of(&e, o);
    let notices = e.take_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(notices[0].kind, NoticeKind::Success);
    assert_eq!(notices[0].title, "Nouvelle pièce créée : Pièce#1");
    assert_eq!(notices[0].action, Some(NoticeAction::Rename(NameTarget::Component(part))));
    assert!(e.take_naming().is_none(), "no dialog by itself");
    assert!(e.quick_card().is_some(), "the card as well");

    // The button: a step of its own.
    let naming = e.naming_of(NameTarget::Component(part)).unwrap();
    assert_eq!((naming.name.as_str(), naming.title.as_str()), ("Pièce#1", "Nom de la pièce"));
    assert_eq!(e.name_refusal(&naming, "  "), Some("le nom est vide".into()));
    assert_eq!(e.name_refusal(&naming, "Pièce#1"), None, "its own name");
    let steps = e.history().journal().count();
    e.give_name(&naming, " Pied ").unwrap();
    assert_eq!(e.document().component(part).unwrap().name, "Pied");
    assert_eq!(e.history().journal().count(), steps + 1);
    e.undo();
    assert_eq!(e.document().component(part).unwrap().name, "Pièce#1");

    // Another part: the first one's name is taken.
    let first = e.naming_of(NameTarget::Component(part)).unwrap();
    e.give_name(&first, "Pied").unwrap();
    let q = box_then_quick(&mut e);
    let naming = e.naming_of(NameTarget::Component(part_of(&e, q))).unwrap();
    assert!(e.name_refusal(&naming, "Pied").is_some());
    assert!(e.give_name(&naming, "Pied").is_err());

    // A copy: no part made, no toast.
    e.take_notices();
    e.set_tool(ToolKind::Select);
    e.apply(Operation::Duplicate { context: e.document().root(), entities: vec![o], transform: glam::DAffine3::IDENTITY }).unwrap();
    assert!(e.take_notices().is_empty());
}

#[test]
fn with_ask_name_the_dialog_opens_and_the_name_joins_the_creation() {
    let mut e = top_editor(Document::new());
    e.set_tool(ToolKind::Box);
    assert!(e.tool_options().iter().any(|o| o.key == tools::ASK_NAME));
    e.set_tool_option(tools::ASK_NAME, OptionValue::Toggle(true));
    e.take_notices();
    let steps = e.history().journal().count();
    let o = box_then_quick(&mut e);
    let part = part_of(&e, o);
    assert!(e.take_notices().is_empty(), "no toast");
    let naming = e.take_naming().expect("the dialog");
    assert_eq!((naming.target, naming.name.as_str()), (NameTarget::Component(part), "Pièce#1"));
    assert!(e.take_naming().is_none(), "taken once");
    e.give_name(&naming, "Côté").unwrap();
    assert_eq!(e.document().component(part).unwrap().name, "Côté");
    assert_eq!(e.history().journal().count(), steps + 1, "one step");
    assert!(e.quick_card().is_some(), "the card still on it");
    e.undo();
    assert!(e.document().component(part).is_err(), "one undo takes the part away");
    e.redo();
    assert_eq!(e.document().component(part).unwrap().name, "Côté");

    // Something done since: a step of its own.
    let o = box_then_quick(&mut e);
    let naming = e.take_naming().unwrap();
    e.apply(Operation::RenameOccurrence { context: e.document().root(), occurrence: o, name: "Gauche".into() }).unwrap();
    let steps = e.history().journal().count();
    e.give_name(&naming, "Dos").unwrap();
    assert_eq!(e.history().journal().count(), steps + 1);
}

#[test]
fn an_assembly_or_a_group_made_is_offered_its_name_by_a_toast() {
    use skraft_model::{Scope, Sign};
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "18");
    let b = draw_box(&mut e, DVec3::new(200.0, 0.0, 0.0), DVec3::new(300.0, 100.0, 0.0), "18");
    e.set_tool(ToolKind::Select);
    e.take_notices();
    e.select(vec![a, b]);
    e.make_assembly();
    let assembly = part_of(&e, e.selection()[0]);
    let notices = e.take_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(notices[0].title, "Nouvel assemblage créé : Assemblage#1");
    let target = NameTarget::Component(assembly);
    assert_eq!(notices[0].action, Some(NoticeAction::Rename(target)));
    let naming = e.naming_of(target).unwrap();
    assert_eq!(naming.title, "Nom de l'assemblage");
    let steps = e.history().journal().count();
    e.give_name(&naming, "Caisson").unwrap();
    assert_eq!(e.document().component(assembly).unwrap().name, "Caisson");
    assert_eq!(e.history().journal().count(), steps + 1, "a step of its own");
    e.undo();
    e.undo();
    assert!(e.naming_of(target).is_none(), "gone with its assembly");

    // In an open part: its bodies grouped, the group's definition offered its name.
    let target = EntityPath::root(a);
    let drill = |x: f64| Operation::AddBody {
        target: target.clone(),
        primitive: Primitive::Cylinder { origin: DVec3::new(x, 50.0, -5.0), radius: 4.0, height: 30.0 },
        placement: glam::DAffine3::IDENTITY,
        sign: Sign::Negative,
        scope: Scope::All,
    };
    let holes = [e.apply(drill(20.0)).unwrap().unwrap().bodies[0], e.apply(drill(40.0)).unwrap().unwrap().bodies[0]];
    e.enter(a);
    e.select(holes.to_vec());
    e.take_notices();
    e.make_assembly();
    let group = e.selection()[0];
    let part = part_of(&e, a);
    let def = e.document().part(part).unwrap().body(group).unwrap().def;
    let notices = e.take_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(notices[0].title, "Nouveau groupe créé : Groupe#1");
    let target = NameTarget::BodyDef { part, def };
    assert_eq!(notices[0].action, Some(NoticeAction::Rename(target)));
    let naming = e.naming_of(target).unwrap();
    assert_eq!(naming.title, "Nom du groupe");
    // A body definition's name need not be unique: only an empty one refused.
    assert_eq!(e.name_refusal(&naming, "Pièce#1"), None);
    assert!(e.name_refusal(&naming, " ").is_some());
    e.give_name(&naming, "Perçages").unwrap();
    assert_eq!(e.document().part(part).unwrap().def(def).unwrap().name, "Perçages");
}

/// Three boxes, the first two put in an assembly: (the editor, the assembly's occurrence, its
/// two parts' occurrences in it, the third box's occurrence at the root).
fn two_in_an_assembly() -> (Editor, Id, [Id; 2], Id) {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "18");
    let b = draw_box(&mut e, DVec3::new(200.0, 0.0, 0.0), DVec3::new(300.0, 100.0, 0.0), "18");
    let c = draw_box(&mut e, DVec3::new(400.0, 0.0, 0.0), DVec3::new(500.0, 100.0, 0.0), "18");
    e.set_tool(ToolKind::Select);
    e.select(vec![a, b]);
    e.make_assembly();
    let asm = e.selection()[0];
    let inner = e.document().assembly(part_of(&e, asm)).unwrap().occurrences().iter().map(|o| o.id).collect::<Vec<_>>();
    e.take_notices();
    (e, asm, [inner[0], inner[1]], c)
}

#[test]
fn under_parts_an_assembly_is_made_in_the_parts_assembly() {
    let (mut e, asm, [a, b], c) = two_in_an_assembly();
    let steps = e.history().journal().count();
    // Not in one assembly: refused, told.
    e.select_paths(vec![EntityPath::new(vec![asm], a), EntityPath::root(c)]);
    e.make_assembly();
    assert_eq!(e.history().journal().count(), steps);
    assert_eq!(e.status(), "Les éléments à regrouper doivent être dans un même assemblage");
    // Both in the same one: a new assembly there, selected by its path, offered its name.
    e.select_paths(vec![EntityPath::new(vec![asm], a), EntityPath::new(vec![asm], b)]);
    e.make_assembly();
    assert_eq!(e.history().journal().count(), steps + 1);
    let outer = e.document().assembly(part_of(&e, asm)).unwrap();
    assert_eq!(outer.occurrences().len(), 1);
    let made = outer.occurrences()[0].clone();
    assert_eq!(e.document().assembly(made.component).unwrap().occurrences().len(), 2);
    assert_eq!(e.deep_selection(), [EntityPath::new(vec![asm], made.id)]);
    let notices = e.take_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(notices[0].action, Some(NoticeAction::Rename(NameTarget::Component(made.component))));
}

#[test]
fn an_assembly_emptied_goes() {
    // Its parts erased from outside (« Pièces »): gone in the same step.
    let (mut e, asm, [a, b], _) = two_in_an_assembly();
    let root = e.document().root();
    let steps = e.history().journal().count();
    e.select_paths(vec![EntityPath::new(vec![asm], a), EntityPath::new(vec![asm], b)]);
    e.delete_selection();
    assert!(e.document().assembly(root).unwrap().occurrence(asm).is_none());
    assert_eq!(e.history().journal().count(), steps + 1);
    e.undo();
    assert_eq!(e.document().assembly(part_of(&e, asm)).unwrap().occurrences().len(), 2, "one undo brings both back");

    // Emptied from inside: kept while edited, gone once left, within the erasing step.
    e.enter(asm);
    e.select_all();
    e.delete_selection();
    assert!(e.document().assembly(root).unwrap().occurrence(asm).is_some(), "kept while edited");
    e.exit();
    assert!(e.document().assembly(root).unwrap().occurrence(asm).is_none(), "gone once left");
    assert!(e.selection().is_empty());
    assert_eq!(e.history().journal().count(), steps + 1);
    e.undo();
    assert_eq!(e.document().assembly(part_of(&e, asm)).unwrap().occurrences().len(), 2);

    // Nested: an assembly holding only the emptied one goes as well, once left in turn.
    e.select(vec![asm]);
    e.make_assembly();
    let top = e.selection()[0];
    e.enter(top);
    e.enter(asm);
    e.select_all();
    e.delete_selection();
    e.exit();
    assert!(e.document().assembly(root).unwrap().occurrence(top).is_some(), "the context, kept");
    e.exit();
    assert!(e.document().assembly(root).unwrap().occurrence(top).is_none());
    e.undo();
    assert!(e.document().assembly(root).unwrap().occurrence(top).is_some(), "one undo brings it all back");
}

#[test]
fn what_is_made_unique_is_offered_its_name_by_a_toast() {
    use skraft_model::Scope;
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "18");
    let root = e.document().root();
    let copy = |e: &mut Editor, o: Id| {
        let op = Operation::Duplicate { context: root, entities: vec![o], transform: glam::DAffine3::from_translation(DVec3::X * 300.0) };
        e.apply(op).unwrap().unwrap().created[0]
    };
    let a2 = copy(&mut e, a);
    e.set_tool(ToolKind::Select);
    // Already unique: nothing made, no toast.
    let b = draw_box(&mut e, DVec3::new(0.0, 200.0, 0.0), DVec3::new(100.0, 300.0, 0.0), "18");
    e.set_tool(ToolKind::Select);
    e.take_notices();
    e.select(vec![b]);
    e.make_unique();
    assert!(e.take_notices().is_empty());
    // One made unique: its toast, a « Renommer » on its new part.
    e.select(vec![a2]);
    e.make_unique();
    let made = part_of(&e, a2);
    assert_ne!(made, part_of(&e, a));
    let notices = e.take_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(notices[0].title, format!("Pièce rendue unique : {}", e.document().component(made).unwrap().name));
    assert_eq!(notices[0].action, Some(NoticeAction::Rename(NameTarget::Component(made))));
    // Two at once: one toast that counts them, no button.
    let c1 = copy(&mut e, a);
    let c2 = copy(&mut e, b);
    e.select(vec![c1, c2]);
    e.make_unique();
    let notices = e.take_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!((notices[0].title.as_str(), notices[0].action), ("2 éléments rendus uniques", None));

    // A body made unique in an open part: its new definition.
    let target = EntityPath::root(a);
    let part = part_of(&e, a);
    let body = e.document().part(part).unwrap().bodies[0].id;
    let op = Operation::DuplicateBodies {
        target,
        bodies: vec![body],
        transform: glam::DAffine3::from_translation(DVec3::Z * 50.0),
        scope: Scope::All,
    };
    let twin = e.apply(op).unwrap().unwrap().bodies[0];
    e.enter(a);
    e.select(vec![twin]);
    e.take_notices();
    e.make_unique();
    let def = e.document().part(part).unwrap().body(twin).unwrap().def;
    let notices = e.take_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(notices[0].action, Some(NoticeAction::Rename(NameTarget::BodyDef { part, def })));
    assert!(notices[0].title.starts_with("Corps rendu unique : "), "{}", notices[0].title);
    assert_eq!(e.naming_of(NameTarget::BodyDef { part, def }).unwrap().title, "Nom du corps");
}

#[test]
fn a_group_of_bodies_opens_like_a_part_and_closes_onto_its_parent() {
    use skraft_model::{Scope, Sign};
    let mut e = top_editor(Document::new());
    let board = draw_box(&mut e, DVec3::ZERO, DVec3::new(200.0, 50.0, 0.0), "18");
    let target = EntityPath::root(board);
    let mut holes = Vec::new();
    for x in [20.0, 40.0] {
        let drill = Primitive::Cylinder { origin: DVec3::new(x, 25.0, -5.0), radius: 4.0, height: 30.0 };
        let op = Operation::AddBody {
            target: target.clone(),
            primitive: drill,
            placement: glam::DAffine3::IDENTITY,
            sign: Sign::Negative,
            scope: Scope::All,
        };
        holes.push(e.apply(op).unwrap().unwrap().bodies[0]);
    }
    e.enter(board);
    e.select(holes.clone());
    assert!(e.can_group_bodies());
    e.group_bodies();
    let group = e.selection()[0];
    // Opened: its two holes, the bodies the tools act on.
    e.enter(group);
    assert_eq!(e.open_group(), [group]);
    let path = e.open_part_path().unwrap();
    assert_eq!(path.group, vec![group]);
    let (_, g, _) = e.document().resolve_container(&path).unwrap();
    assert_eq!(g.bodies.iter().map(|b| b.id).collect::<Vec<_>>(), holes);
    assert_eq!(e.scene().recipe().unwrap().bodies().count(), 2);
    // One erased inside it: the group keeps the other.
    e.select(vec![holes[0]]);
    e.delete_selection();
    let (_, g, _) = e.document().resolve_container(&path).unwrap();
    assert_eq!(g.bodies.len(), 1);
    assert_eq!(e.document().resolve_part(&target).unwrap().1.bodies.len(), 2);
    // Out of it: in the part, the group selected; then out of the part.
    e.exit();
    assert!(e.open_group().is_empty() && e.open_part() == Some(board));
    assert_eq!(e.selection(), [group]);
    e.exit();
    assert_eq!(e.open_part(), None);
    // Undone back to before the group: the group open is closed.
    e.enter(board);
    e.enter(group);
    e.undo();
    e.undo();
    assert!(e.open_group().is_empty());
    assert_eq!(e.open_part(), Some(board));
}

#[test]
fn machinings_inside_groups_show_with_their_group() {
    use skraft_model::{Scope, Sign};
    let mut e = top_editor(Document::new());
    let board = draw_box(&mut e, DVec3::ZERO, DVec3::new(200.0, 50.0, 0.0), "18");
    let target = EntityPath::root(board);
    let add = |e: &mut Editor, primitive: Primitive, sign: Sign| {
        let op = Operation::AddBody { target: target.clone(), primitive, placement: glam::DAffine3::IDENTITY, sign, scope: Scope::All };
        e.apply(op).unwrap().unwrap().bodies[0]
    };
    let block = add(&mut e, Primitive::Box { origin: DVec3::new(50.0, 10.0, 18.0), size: DVec3::new(30.0, 30.0, 20.0) }, Sign::Positive);
    let drill = add(&mut e, Primitive::Cylinder { origin: DVec3::new(65.0, 25.0, 20.0), radius: 4.0, height: 30.0 }, Sign::Negative);
    let a = add(&mut e, Primitive::Cylinder { origin: DVec3::new(150.0, 25.0, -5.0), radius: 4.0, height: 30.0 }, Sign::Negative);
    let b = add(&mut e, Primitive::Cylinder { origin: DVec3::new(150.0, 25.0, 12.0), radius: 8.0, height: 10.0 }, Sign::Negative);
    e.enter(board);
    // The block and its drill: a positive group; the counterbore: a negative one.
    e.select(vec![block, drill]);
    e.group_bodies();
    let group = e.selection()[0];
    e.select(vec![a, b]);
    e.group_bodies();
    let r = e.scene().recipe().unwrap();
    // The drill shows, with its group (not picked); nothing of the counterbore's inside.
    assert_eq!(r.nested.len(), 1);
    assert_eq!(r.nested[0].body, group);
    assert!((r.nested[0].placement.translation - DVec3::new(65.0, 25.0, 20.0)).length() < 1e-9);
    assert_eq!(r.bodies().count(), 3);
}

#[test]
fn previous_and_next_view_go_through_the_views() {
    let mut e = Editor::new(Document::new(), Box::new(skraft_kernel::Inline::default()));
    e.set_viewport_size(DVec2::new(1000.0, 1000.0));
    assert!(!e.previous_view(), "nothing before the first view");
    let first = e.camera.clone();
    // An orbit, then a pan: two gestures, each from where the last one ended.
    e.nav_begin(DVec2::new(500.0, 500.0));
    e.orbit(DVec2::new(80.0, 0.0));
    let orbited = e.camera.clone();
    e.nav_begin(DVec2::new(500.0, 500.0));
    e.pan(DVec2::new(30.0, 10.0));
    let panned = e.camera.clone();
    assert!(e.previous_view());
    assert_eq!(e.camera, orbited);
    assert!(e.previous_view());
    assert_eq!(e.camera, first);
    assert!(!e.previous_view());
    assert!(e.next_view());
    assert_eq!(e.camera, orbited);
    assert!(e.next_view());
    assert_eq!(e.camera, panned);
    assert!(!e.next_view());
    // Gone back, then moved: the views after it are dropped.
    assert!(e.previous_view());
    e.zoom_extents();
    assert!(!e.next_view());
    assert!(e.previous_view());
    assert_eq!(e.camera, orbited);
    // At most 50 views.
    for _ in 0..60 {
        e.nav_begin(DVec2::new(500.0, 500.0));
        e.orbit(DVec2::new(5.0, 0.0));
    }
    assert!(e.previous_view());
    assert_eq!(e.views().0.len(), 50);
}

#[test]
fn the_pivot_is_the_style_s() {
    use crate::navigation::Pivot;
    let mut e = top_editor(Document::new());
    draw_box(&mut e, DVec3::new(200.0, 0.0, 0.0), DVec3::new(300.0, 100.0, 0.0), "100");
    e.set_tool(ToolKind::Select);
    e.select(Vec::new());
    // The model's centre, nothing selected; the screen's centre.
    e.nav_begin_about(DVec2::ZERO, Pivot::SelectionOrModel);
    assert!((e.pivot() - DVec3::new(250.0, 50.0, 50.0)).length() < 1e-3, "{}", e.pivot());
    e.nav_begin_about(DVec2::ZERO, Pivot::ScreenCentre);
    let target = e.camera.eye + e.camera.forward() * e.camera.focus;
    assert!((e.pivot() - target).length() < 1e-9);
}

#[test]
fn move_picks_first_by_a_click_a_rectangle_or_shift_clicks() {
    let (mut e, [b1, b2, b3]) = three_boards_editor();
    let none = Modifiers::default();
    let shift = Modifiers { toggle: true, copy: false };
    let x = |e: &Editor, o: Id| bounds_of(e, &EntityPath::root(o)).0.x;
    e.select(vec![]);
    e.set_tool(ToolKind::Move);

    // Nothing selected: the tool picks (docs/tools/selection.md); a click takes one, the next starts.
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(100.0, 25.0, 18.0)) });
    assert!(e.status().contains("à déplacer"), "{}", e.status());
    assert_eq!(e.view().highlights.iter().filter(|h| h.mark == Mark::Hover).count(), 1);
    click(&mut e, DVec3::new(100.0, 25.0, 18.0));
    assert_eq!(e.selection(), [b1]);
    assert!(e.status().contains("point de départ"), "{}", e.status());
    // Esc while moving goes back to the start, the selection kept; Esc again lets it go.
    click(&mut e, DVec3::new(100.0, 25.0, 18.0));
    e.input(ToolInput::Key { key: Key::Escape, mods: none });
    assert_eq!(e.selection(), [b1]);
    e.input(ToolInput::Key { key: Key::Escape, mods: none });
    assert!(e.selection().is_empty());
    assert_eq!(e.tool_kind(), ToolKind::Move);

    // A rectangle around the first two, left to right: taken; moved 50 along red, let go.
    drag(&mut e, DVec3::new(-10.0, -10.0, 0.0), DVec3::new(210.0, 160.0, 0.0), none);
    assert_eq!(e.selection(), [b1, b2]);
    click(&mut e, DVec3::new(0.0, 0.0, 18.0));
    e.input(ToolInput::Key { key: Key::Right, mods: none });
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(100.0, 0.0, 18.0)) });
    e.input(ToolInput::Vcb("50".into()));
    assert_eq!((x(&e, b1).round(), x(&e, b2).round(), x(&e, b3).round()), (50.0, 50.0, 0.0));
    assert!(e.selection().is_empty());

    // Shift held, two clicked: gathered, taken once Shift is released.
    e.set_shift(true);
    for p in [DVec3::new(150.0, 25.0, 18.0), DVec3::new(100.0, 325.0, 18.0)] {
        e.input(ToolInput::Hover { px: px(&e, p) });
        e.input(ToolInput::Click { px: px(&e, p), mods: shift });
    }
    assert_eq!(e.selection(), [b1, b3]);
    assert!(e.status().contains("Sélection multiple"), "{}", e.status());
    e.set_shift(false);
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(100.0, 325.0, 18.0)) });
    assert!(e.status().contains("point de départ"), "{}", e.status());
}

#[test]
fn rotate_picks_first() {
    let (mut e, [b1, ..]) = three_boards_editor();
    e.select(vec![]);
    e.set_tool(ToolKind::Rotate);
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(100.0, 25.0, 18.0)) });
    assert!(e.status().contains("à faire pivoter"), "{}", e.status());
    // No protractor while picking.
    assert!(e.preview().drawing.lines.len() < 40);
    click(&mut e, DVec3::new(100.0, 25.0, 18.0));
    assert_eq!(e.selection(), [b1]);
    assert!(e.status().contains("centre de rotation"), "{}", e.status());
}

/// Two parts in an assembly placed twice (the second 300 along green), and a part on its own.
fn drawers_editor() -> (Editor, [Id; 5]) {
    let mut e = top_editor(Document::new());
    let root = e.document().root();
    let add = |e: &mut Editor, origin: DVec3| {
        let p = Primitive::Box { origin, size: DVec3::new(100.0, 100.0, 50.0) };
        e.apply(Operation::AddPrimitive { context: root, primitive: p, placement: glam::DAffine3::IDENTITY, material: None })
            .unwrap()
            .unwrap()
            .created[0]
    };
    let a = add(&mut e, DVec3::ZERO);
    let b = add(&mut e, DVec3::new(200.0, 0.0, 0.0));
    let c = add(&mut e, DVec3::new(600.0, 0.0, 0.0));
    let g = e.apply(Operation::MakeAssembly { context: root, entities: vec![a, b], name: None }).unwrap().unwrap().created[0];
    let t = glam::DAffine3::from_translation(DVec3::Y * 300.0);
    let g2 = e.apply(Operation::Duplicate { context: root, entities: vec![g], transform: t }).unwrap().unwrap().created[0];
    e.select(vec![]);
    (e, [a, b, c, g, g2])
}

fn parts_scope(e: &mut Editor) {
    e.set_tool_option(crate::tools::PICK_SCOPE, OptionValue::Choice(crate::tools::PICK_PARTS.into()));
    assert!(e.pick_parts());
}

#[test]
fn parts_are_selected_at_any_depth_without_entering() {
    let (mut e, [a, b, c, g, g2]) = drawers_editor();
    let none = Modifiers::default();
    let shift = Modifiers { toggle: true, copy: false };
    // The scope shows in Select's bar, last.
    assert!(e.tool_options().last().is_some_and(|o| o.key == crate::tools::PICK_SCOPE));
    parts_scope(&mut e);

    // A click takes the part, in its assembly; the context stays the model's.
    click(&mut e, DVec3::new(50.0, 50.0, 50.0));
    assert_eq!(e.deep_selection(), [EntityPath::new(vec![g], a)]);
    assert!(e.selection().is_empty());
    assert!(e.context().is_empty());
    assert!(e.view().highlights.iter().any(|h| h.mark == Mark::Selected));
    // Shift adds one of the other copy; a part of the model is one too.
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(250.0, 350.0, 50.0)) });
    e.input(ToolInput::Click { px: px(&e, DVec3::new(250.0, 350.0, 50.0)), mods: shift });
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(650.0, 50.0, 50.0)) });
    e.input(ToolInput::Click { px: px(&e, DVec3::new(650.0, 50.0, 50.0)), mods: shift });
    assert_eq!(e.deep_selection(), [EntityPath::new(vec![g], a), EntityPath::new(vec![g2], b), EntityPath::root(c)]);
    assert!(e.deep_shared(), "through an assembly placed twice");
    // Esc lets them go.
    e.input(ToolInput::Key { key: Key::Escape, mods: none });
    assert!(e.deep_selection().is_empty());

    // A rectangle around the second copy, left to right: its two parts.
    drag(&mut e, DVec3::new(-10.0, 410.0, 0.0), DVec3::new(310.0, 290.0, 0.0), none);
    assert_eq!(e.deep_selection(), [EntityPath::new(vec![g2], a), EntityPath::new(vec![g2], b)]);
    // Back to « Context »: let go; a click takes the assembly.
    e.set_tool_option(crate::tools::PICK_SCOPE, OptionValue::Choice(crate::tools::PICK_CONTEXT.into()));
    assert!(e.deep_selection().is_empty());
    click(&mut e, DVec3::new(50.0, 50.0, 50.0));
    assert_eq!(e.selection(), [g]);
}

#[test]
fn guides_are_selected_with_parts_at_any_depth() {
    use skraft_model::{GuidePath, GuideShape};
    let (mut e, [a, _, c, g, g2]) = drawers_editor();
    let root = e.document().root();
    let line = |y: f64, z: f64| GuideShape::Line { origin: DVec3::new(0.0, y, z), dir: DVec3::X };
    let add = |e: &mut Editor, component: Id, shape| e.apply(Operation::AddGuide { component, shape }).unwrap().unwrap().guide.unwrap();
    let assembly = e.document().resolve(&EntityPath::root(g)).unwrap().component;
    let part = e.document().resolve(&EntityPath::new(vec![g], a)).unwrap().component;
    // The model's at -300; the assembly's at -100 in its axes (200 in its copy); part a's at
    // 80, above it (380 in the copy).
    let model_guide = add(&mut e, root, line(-300.0, 0.0));
    let assembly_guide = add(&mut e, assembly, line(-100.0, 0.0));
    let part_guide = add(&mut e, part, line(80.0, 100.0));
    let y_of = |e: &Editor, component: Id, id: Id| {
        let GuideShape::Line { origin, .. } = e.document().component(component).unwrap().guide(id).unwrap().shape else { panic!() };
        origin.y
    };
    let count = |e: &Editor, component: Id| e.document().component(component).unwrap().guides.len();
    let none = Modifiers::default();
    let shift = Modifiers { toggle: true, copy: false };
    let shift_click = |e: &mut Editor, p: DVec3| {
        e.input(ToolInput::Hover { px: px(e, p) });
        e.input(ToolInput::Click { px: px(e, p), mods: shift });
    };

    // Under « Context », the deeper guides are faded; under « Parts », half faded.
    assert!(guides_in(&e.view(), ThemeColor::HalfFadedGuide).is_empty());
    parts_scope(&mut e);
    assert_eq!(guides_in(&e.view(), ThemeColor::HalfFadedGuide).len(), 4, "the assembly's and the part's, in both copies");

    // A click near a guide takes it, at any depth, hovered first; Shift adds guides and parts.
    hover(&mut e, DVec3::new(100.0, 203.0, 0.0));
    assert!(!guides_in(&e.view(), ThemeColor::Hover).is_empty());
    click(&mut e, DVec3::new(100.0, 203.0, 0.0));
    assert_eq!(e.deep_guides(), [GuidePath::new(vec![g2], assembly_guide)]);
    assert!(e.selection().is_empty() && e.deep_selection().is_empty());
    shift_click(&mut e, DVec3::new(100.0, -303.0, 0.0));
    shift_click(&mut e, DVec3::new(100.0, 83.0, 100.0));
    shift_click(&mut e, DVec3::new(650.0, 50.0, 50.0));
    assert_eq!(
        e.deep_guides(),
        [GuidePath::new(vec![g2], assembly_guide), GuidePath::root(model_guide), GuidePath::new(vec![g, a], part_guide)]
    );
    assert_eq!(e.deep_selection(), [EntityPath::root(c)]);
    assert!(!guides_in(&e.view(), ThemeColor::Selected).is_empty());
    // Shift takes one out.
    shift_click(&mut e, DVec3::new(100.0, -303.0, 0.0));
    assert_eq!(e.deep_guides().len(), 2);
    shift_click(&mut e, DVec3::new(100.0, -303.0, 0.0));

    // Copied together: the three guides in the clip.
    let clip = e.copy().expect("copied");
    let crate::Content::Entities(f) = &clip.content else { panic!() };
    assert_eq!(f.component(f.root()).unwrap().guides.len(), 3);

    // Moved together 100 along green, each in its component (the assembly's and the part's in
    // both copies), in one step.
    e.set_tool(ToolKind::Move);
    click(&mut e, DVec3::new(650.0, 50.0, 50.0));
    hover(&mut e, DVec3::new(650.0, 150.0, 50.0));
    e.input(ToolInput::Vcb("100".into()));
    assert!((y_of(&e, root, model_guide) + 200.0).abs() < 1e-6);
    assert!(y_of(&e, assembly, assembly_guide).abs() < 1e-6);
    assert!((y_of(&e, part, part_guide) - 180.0).abs() < 1e-6);
    assert_eq!(bounds_of(&e, &EntityPath::root(c)).0.y.round(), 100.0);
    e.undo();
    assert!((y_of(&e, assembly, assembly_guide) + 100.0).abs() < 1e-6);

    // The assembly's guide taken in both copies: moved once.
    e.set_tool(ToolKind::Select);
    click(&mut e, DVec3::new(100.0, 203.0, 0.0));
    shift_click(&mut e, DVec3::new(100.0, -97.0, 0.0));
    assert_eq!(e.deep_guides().len(), 2);
    e.set_tool(ToolKind::Move);
    click(&mut e, DVec3::new(100.0, -100.0, 0.0));
    e.input(ToolInput::Key { key: Key::Left, mods: none });
    hover(&mut e, DVec3::new(100.0, -20.0, 0.0));
    e.input(ToolInput::Vcb("100".into()));
    assert!(y_of(&e, assembly, assembly_guide).abs() < 1e-6, "{}", y_of(&e, assembly, assembly_guide));
    e.undo();

    // Deleted together, in one step.
    e.set_tool(ToolKind::Select);
    click(&mut e, DVec3::new(100.0, 83.0, 100.0));
    shift_click(&mut e, DVec3::new(100.0, -303.0, 0.0));
    e.delete_selection();
    assert_eq!((count(&e, root), count(&e, part)), (0, 0));
    e.undo();
    assert_eq!((count(&e, root), count(&e, part)), (1, 1));

    // Erase: a click near a guide at any depth erases it.
    e.select(vec![]);
    e.set_tool(ToolKind::Erase);
    hover(&mut e, DVec3::new(100.0, 203.0, 0.0));
    click(&mut e, DVec3::new(100.0, 203.0, 0.0));
    assert_eq!(count(&e, assembly), 0);
}

#[test]
fn parts_at_any_depth_move_in_their_assembly_all_copies_or_this_one() {
    let (mut e, [a, _, c, g, g2]) = drawers_editor();
    let none = Modifiers::default();
    let x = |e: &Editor, path: EntityPath| bounds_of(e, &path).0.x;
    parts_scope(&mut e);
    e.set_tool(ToolKind::Move);
    assert!(e.tool_options().iter().any(|o| o.key == crate::tools::PICK_SCOPE), "shared by the tools that pick parts");
    assert!(e.pick_parts(), "one setting for all");

    // Picked, then moved 100 along red: the assembly's definition changes, both copies.
    click(&mut e, DVec3::new(50.0, 50.0, 50.0));
    assert_eq!(e.deep_selection(), [EntityPath::new(vec![g], a)]);
    click(&mut e, DVec3::new(50.0, 50.0, 50.0));
    e.input(ToolInput::Key { key: Key::Right, mods: none });
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(150.0, 50.0, 50.0)) });
    assert!(e.view().highlights.iter().any(|h| h.mark == Mark::Ghost));
    e.input(ToolInput::Vcb("100".into()));
    assert_eq!(x(&e, EntityPath::new(vec![g], a)).round(), 100.0);
    assert_eq!(x(&e, EntityPath::new(vec![g2], a)).round(), 100.0);
    assert!(e.deep_selection().is_empty(), "moved, let go");
    e.undo();

    // « Only this one »: the copy reached made unique, the other stays; one undo step.
    e.set_part_scope(skraft_model::Scope::This);
    click(&mut e, DVec3::new(50.0, 50.0, 50.0));
    click(&mut e, DVec3::new(50.0, 50.0, 50.0));
    e.input(ToolInput::Key { key: Key::Right, mods: none });
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(150.0, 50.0, 50.0)) });
    e.input(ToolInput::Vcb("100".into()));
    assert_eq!(x(&e, EntityPath::new(vec![g], a)).round(), 100.0);
    assert_eq!(x(&e, EntityPath::new(vec![g2], a)).round(), 0.0);
    let comp = |e: &Editor, o: Id| e.document().resolve(&EntityPath::root(o)).unwrap().component;
    assert_ne!(comp(&e, g), comp(&e, g2));
    e.undo();
    assert_eq!(comp(&e, g), comp(&e, g2));

    // Delete erases parts at any depth, each from its assembly, in one step.
    e.set_tool(ToolKind::Select);
    click(&mut e, DVec3::new(50.0, 350.0, 50.0));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(650.0, 50.0, 50.0)) });
    e.input(ToolInput::Click { px: px(&e, DVec3::new(650.0, 50.0, 50.0)), mods: Modifiers { toggle: true, copy: false } });
    assert_eq!(e.deep_selection().len(), 2);
    e.input(ToolInput::Key { key: Key::Delete, mods: none });
    assert!(e.document().resolve(&EntityPath::root(c)).is_err());
    assert!(e.document().resolve(&EntityPath::new(vec![g2], a)).is_err(), "only this one: the copy");
    assert!(e.document().resolve(&EntityPath::new(vec![g], a)).is_ok());
    e.undo();
    assert!(e.document().resolve(&EntityPath::root(c)).is_ok());
}

#[test]
fn copy_tools_stretch_paint_and_the_clipboard_take_parts_at_any_depth() {
    let (mut e, [a, b, _, g, g2]) = drawers_editor();
    let none = Modifiers::default();
    let in_g =
        |e: &Editor| e.document().assembly(e.document().resolve(&EntityPath::root(g)).unwrap().component).unwrap().occurrences().len();
    parts_scope(&mut e);

    // Copy along a line: the part picked in its assembly, copied there (both copies of it).
    e.set_tool(ToolKind::CopyLine);
    click(&mut e, DVec3::new(50.0, 50.0, 50.0));
    assert_eq!(e.deep_selection(), [EntityPath::new(vec![g], a)]);
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(50.0, 200.0, 50.0)) });
    e.input(ToolInput::Click { px: px(&e, DVec3::new(50.0, 200.0, 50.0)), mods: none });
    assert_eq!(in_g(&e), 3, "{}", e.status());
    assert!(e.deep_selection().is_empty(), "the copies let go");
    e.undo();
    assert_eq!(in_g(&e), 2);

    // The Stretch: parts of one assembly are its target; of two, it says so.
    e.set_tool(ToolKind::Select);
    click(&mut e, DVec3::new(50.0, 350.0, 50.0));
    e.set_tool(ToolKind::Stretch);
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(900.0, 900.0, 0.0)) });
    assert!(e.status().contains("Cliquer une poignée"), "{}", e.status());
    e.select_paths(vec![EntityPath::new(vec![g], a), EntityPath::new(vec![g2], b)]);
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(900.0, 900.0, 0.0)) });
    assert!(e.status().contains("même assemblage"), "{}", e.status());

    // The clipboard: both copied, in world axes, ids made distinct.
    let clip = e.copy().expect("copied");
    let Content::Entities(f) = &clip.content else { panic!("entities") };
    assert_eq!(f.root_assembly().occurrences().len(), 2);

    // Paint: the part under the cursor, in its assembly (the definition shared: both copies).
    e.set_tool(ToolKind::Select);
    e.input(ToolInput::Key { key: Key::Escape, mods: none });
    let oak = e.apply(Operation::AddMaterial { name: "Chêne".into(), color: [180, 140, 90] }).unwrap().unwrap().material.unwrap();
    e.set_paint_material(Some(oak));
    e.set_tool(ToolKind::Paint);
    click(&mut e, DVec3::new(250.0, 350.0, 50.0));
    let material = |e: &Editor, p: EntityPath| e.document().resolve(&p).unwrap().material;
    assert_eq!(material(&e, EntityPath::new(vec![g2], b)), Some(oak));
    assert_eq!(material(&e, EntityPath::new(vec![g], b)), Some(oak));
    assert_eq!(material(&e, EntityPath::root(g2)), None);
}

#[test]
fn the_copy_modifier_held_over_a_part_lists_its_path_to_choose_a_parent() {
    let (mut e, [a, _, c, g, _]) = drawers_editor();
    parts_scope(&mut e);
    e.set_tool(ToolKind::Move);
    // Over a part of the model: no list (nothing above it to choose).
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(650.0, 50.0, 50.0)) });
    e.set_copy(true);
    assert!(e.tree().is_none());
    e.set_copy(false);
    let _ = c;

    // Over a part of an assembly: its path, the model's level first.
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(50.0, 50.0, 50.0)) });
    assert!(e.status().contains("élément parent"), "{}", e.status());
    e.set_copy(true);
    let rows: Vec<EntityPath> = e.tree().unwrap().rows.iter().map(|r| r.path.clone()).collect();
    assert_eq!(rows, [EntityPath::root(g), EntityPath::new(vec![g], a)]);
    assert!(e.status().contains("Choisir un élément"), "{}", e.status());
    // A row hovered is drawn hovered; clicked, the assembly is selected and taken.
    e.hover_tree_row(Some(0));
    assert!(e.view().highlights.iter().any(|h| h.mark == Mark::Hover));
    e.pick_tree_row(0);
    assert!(e.tree().is_none());
    assert_eq!(e.deep_selection(), [EntityPath::root(g)]);
    assert!(e.status().contains("point de départ"), "{}", e.status());
    // The key released: its tap is not the tool's (no copy toggled).
    e.set_copy(false);
    e.input(ToolInput::Key { key: Key::Copy, mods: Modifiers::default() });
    assert!(!e.tool_copying());
}

#[test]
fn the_copy_tools_show_the_part_hovered_at_any_depth_while_picking() {
    let (mut e, _) = drawers_editor();
    parts_scope(&mut e);
    for tool in [ToolKind::CopyLine, ToolKind::CopyGrid, ToolKind::Distribute] {
        e.set_tool(tool);
        e.input(ToolInput::Hover { px: px(&e, DVec3::new(50.0, 50.0, 50.0)) });
        assert!(e.view().highlights.iter().any(|h| h.mark == Mark::Hover), "{tool:?}");
    }
}

#[test]
fn a_click_on_nothing_lets_the_parts_at_any_depth_go() {
    let (mut e, [a, _, _, g, _]) = drawers_editor();
    parts_scope(&mut e);
    click(&mut e, DVec3::new(50.0, 50.0, 50.0));
    assert_eq!(e.deep_selection(), [EntityPath::new(vec![g], a)]);
    click(&mut e, DVec3::new(-800.0, -800.0, 0.0));
    assert!(e.deep_selection().is_empty());
    assert!(!e.view().highlights.iter().any(|h| h.mark == Mark::Selected));
}

#[test]
fn paint_paints_what_a_rectangle_takes_and_the_level_chosen_in_the_tree() {
    let (mut e, [a, b, c, g, g2]) = drawers_editor();
    let none = Modifiers::default();
    let oak = e.apply(Operation::AddMaterial { name: "Chêne".into(), color: [180, 140, 90] }).unwrap().unwrap().material.unwrap();
    e.set_paint_material(Some(oak));
    let material = |e: &Editor, p: EntityPath| e.document().resolve(&p).unwrap().material;
    e.set_tool(ToolKind::Paint);

    // « Context »: a rectangle around the first copy and the lone part, left to right: both
    // painted, in one step.
    let steps = e.history().journal().count();
    drag(&mut e, DVec3::new(-10.0, 110.0, 0.0), DVec3::new(710.0, -10.0, 0.0), none);
    assert_eq!(material(&e, EntityPath::root(g)), Some(oak));
    assert_eq!(material(&e, EntityPath::root(c)), Some(oak));
    assert_eq!(material(&e, EntityPath::root(g2)), None);
    assert_eq!(e.history().journal().count(), steps + 1);
    e.undo();

    // « Parts »: the parts the rectangle takes, at any depth.
    parts_scope(&mut e);
    drag(&mut e, DVec3::new(-10.0, 410.0, 0.0), DVec3::new(310.0, 290.0, 0.0), none);
    assert_eq!(material(&e, EntityPath::new(vec![g2], a)), Some(oak));
    assert_eq!(material(&e, EntityPath::new(vec![g2], b)), Some(oak));
    assert_eq!(material(&e, EntityPath::root(g2)), None);
    e.undo();

    // The tree drop-up: the level chosen is painted, not selected.
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(50.0, 350.0, 50.0)) });
    e.set_copy(true);
    assert!(e.tree().is_some());
    e.pick_tree_row(0);
    e.set_copy(false);
    assert_eq!(material(&e, EntityPath::root(g2)), Some(oak));
    assert!(e.deep_selection().is_empty());
}

/// « Dessiner dans » and « Pipette » (docs/tools/draw-options.md): the drawers' first part `a` (in the
/// assembly `g`, placed twice) painted oak; the box tool with both options on.
fn draw_in_editor() -> (Editor, [Id; 5], Id) {
    let (mut e, ids) = drawers_editor();
    let [a, _, _, g, _] = ids;
    let oak = e.apply(Operation::AddMaterial { name: "Chêne".into(), color: [180, 140, 90] }).unwrap().unwrap().material.unwrap();
    let paint = Operation::PaintOccurrences { context: Id(0), entities: vec![a], material: Some(oak) };
    e.apply(Operation::At { parent: vec![g], unique: false, op: Box::new(paint) }).unwrap();
    e.set_tool(ToolKind::Box);
    e.set_tool_option(crate::tools::DRAW_IN, OptionValue::Toggle(true));
    e.set_tool_option(crate::tools::PICK_MATERIAL, OptionValue::Toggle(true));
    (e, ids, oak)
}

/// A box drawn on the top of `a` (50 high), 10 high.
fn box_on_a(e: &mut Editor) {
    click(e, DVec3::new(20.0, 20.0, 50.0));
    click(e, DVec3::new(80.0, 80.0, 50.0));
    e.input(ToolInput::Vcb("10".into()));
}

#[test]
fn draw_in_makes_the_part_in_the_assembly_of_the_part_picked_with_its_material() {
    let (mut e, [a, b, _, g, g2], oak) = draw_in_editor();
    let option = |e: &Editor, key: &str| e.tool_options().into_iter().find(|o| o.key == key).unwrap();
    // Over `a`: « Pipette » shows its material, its box drawn.
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(20.0, 20.0, 50.0)) });
    let pick = option(&e, crate::tools::PICK_MATERIAL);
    assert_eq!(pick.swatch, Some([180, 140, 90]));
    assert_eq!(pick.label, "Pipette : Chêne");
    // Its box (the assembly's, 25.4 larger), as OCL's: dotted, and its corners bolder, the
    // edges of its faces turned to the eye over the scene (`shell_on_top`, the renderer's).
    let preview = e.preview();
    let grey = crate::draw::Color::Rgba([0.25, 0.25, 0.25, 1.0]);
    let boxes: Vec<_> = preview.drawing.boxes.iter().filter(|b| b.pen.color == grey).collect();
    assert_eq!(boxes.len(), 2, "the box and its corners");
    assert!(boxes.iter().all(|b| b.shell_on_top && (b.bounds.lo.z + 25.4).abs() < 1e-6));
    assert_eq!(boxes[0].pen.stipple, crate::draw::Stipple::Dotted);
    assert!(boxes[0].corners.is_none() && boxes[1].corners.is_some());
    // Over the void: nothing picked.
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(-500.0, -500.0, 0.0)) });
    assert_eq!(option(&e, crate::tools::PICK_MATERIAL).swatch, None);

    e.take_notices();
    box_on_a(&mut e);
    let asm = e.document().resolve(&EntityPath::root(g)).unwrap().component;
    let occs = e.document().assembly(asm).unwrap().occurrences().to_vec();
    assert_eq!(occs.len(), 3, "made in the assembly");
    let made = occs.iter().find(|o| o.id != a && o.id != b).unwrap();
    assert_eq!(made.material, Some(oak));
    assert_eq!(e.document().assembly(e.document().root()).unwrap().occurrences().len(), 3, "not in the model");
    // Placed where it was drawn, in both copies of the assembly (« Portée » all).
    let path = EntityPath::new(vec![g], made.id);
    let (lo, hi) = bounds_of(&e, &path);
    assert!((lo.z - 50.0).abs() < 1e-6 && (hi.z - 60.0).abs() < 1e-6, "on a: {lo} {hi}");
    assert!(lo.x >= -1e-6 && hi.x <= 100.0 + 1e-6 && lo.y >= -1e-6 && hi.y <= 100.0 + 1e-6, "over a: {lo} {hi}");
    let (lo2, _) = bounds_of(&e, &EntityPath::new(vec![g2], made.id));
    assert!(close(lo2, lo + DVec3::Y * 300.0), "{lo2}");
    // Its name offered, the card on it (selected there, at any depth).
    let notices = e.take_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(e.quick_card().is_some());
    e.start_quick_tool(ToolKind::Move);
    assert_eq!(e.deep_selection(), [path]);
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });

    // One undo takes it away.
    e.set_tool(ToolKind::Box);
    e.undo();
    assert_eq!(e.document().assembly(asm).unwrap().occurrences().len(), 2);
}

#[test]
fn draw_in_only_this_copy_makes_the_assembly_unique_first() {
    let (mut e, [_, _, _, g, g2], _) = draw_in_editor();
    e.set_part_scope(skraft_model::Scope::This);
    box_on_a(&mut e);
    let doc = e.document();
    let (one, two) = (doc.resolve(&EntityPath::root(g)).unwrap().component, doc.resolve(&EntityPath::root(g2)).unwrap().component);
    assert_ne!(one, two, "made unique");
    assert_eq!(doc.assembly(one).unwrap().occurrences().len(), 3);
    assert_eq!(doc.assembly(two).unwrap().occurrences().len(), 2);
}

#[test]
fn without_the_options_or_over_a_part_of_the_context_the_part_goes_in_the_context() {
    let (mut e, [_, _, c, g, _], oak) = draw_in_editor();
    let root = e.document().root();
    let asm = e.document().resolve(&EntityPath::root(g)).unwrap().component;
    // « Pipette » alone: in the model, oak.
    e.set_tool_option(crate::tools::DRAW_IN, OptionValue::Toggle(false));
    box_on_a(&mut e);
    assert_eq!(e.document().assembly(asm).unwrap().occurrences().len(), 2);
    let made = drawn(&e);
    assert_eq!(e.document().resolve(&EntityPath::root(made)).unwrap().material, Some(oak));
    // « Dessiner dans » over a part of the model: in the model, no At.
    e.set_tool_option(crate::tools::DRAW_IN, OptionValue::Toggle(true));
    e.set_tool_option(crate::tools::PICK_MATERIAL, OptionValue::Toggle(false));
    let before = e.document().assembly(root).unwrap().occurrences().len();
    click(&mut e, DVec3::new(620.0, 20.0, 50.0));
    click(&mut e, DVec3::new(680.0, 80.0, 50.0));
    e.input(ToolInput::Vcb("10".into()));
    assert_eq!(e.document().assembly(root).unwrap().occurrences().len(), before + 1);
    assert_eq!(e.document().resolve(&EntityPath::root(drawn(&e))).unwrap().material, None);
    let _ = c;
}

#[test]
fn the_part_options_are_not_offered_in_an_open_part() {
    let (mut e, [_, _, c, _, _], _) = draw_in_editor();
    for kind in [ToolKind::Box, ToolKind::Cylinder, ToolKind::Prism] {
        e.set_tool(kind);
        let keys: Vec<_> = e.tool_options().iter().map(|o| o.key).collect();
        assert!(keys.contains(&crate::tools::DRAW_IN) && keys.contains(&crate::tools::PICK_MATERIAL), "{kind:?}");
    }
    e.enter(c);
    assert!(e.open_part().is_some());
    e.set_tool(ToolKind::Box);
    assert!(!e.tool_options().iter().any(|o| o.key == crate::tools::DRAW_IN || o.key == crate::tools::PICK_MATERIAL));
}

#[test]
fn the_part_tools_share_their_settings_as_ocl() {
    let mut e = top_editor(Document::new());
    let value = |e: &Editor, kind: ToolKind, key: &str| e.tool_settings().get(kind.id()).and_then(|s| s.get(key)).cloned();
    let on = Some(OptionValue::Toggle(true));
    e.set_tool(ToolKind::Box);
    e.set_tool_option(crate::tools::DRAW_IN, OptionValue::Toggle(true));
    e.set_tool_option(crate::tools::PICK_MATERIAL, OptionValue::Toggle(true));
    e.set_tool_option("pull_centered", OptionValue::Toggle(true));
    e.set_tool_option("centered", OptionValue::Toggle(true));
    for kind in [ToolKind::Cylinder, ToolKind::Prism, ToolKind::Turn] {
        assert_eq!(value(&e, kind, crate::tools::DRAW_IN), on, "{kind:?}");
        assert_eq!(value(&e, kind, crate::tools::PICK_MATERIAL), on, "{kind:?}");
        assert_eq!(value(&e, kind, "centered"), None, "the box's own");
    }
    assert_eq!(value(&e, ToolKind::Prism, "pull_centered"), on);
    assert_eq!(value(&e, ToolKind::Turn, "pull_centered"), None, "Turn has no centred height");

    // From another tool: the offset typed in the cylinder's bar, the naming in Turn's.
    e.set_tool(ToolKind::Cylinder);
    e.type_tool_length("offset", "3");
    assert_eq!(value(&e, ToolKind::Box, "offset"), Some(OptionValue::Length(3.0)));
    assert_eq!(value(&e, ToolKind::Prism, "offset"), Some(OptionValue::Length(3.0)));
    assert_eq!(value(&e, ToolKind::Turn, "offset"), None, "Turn has no offset");
    e.set_tool(ToolKind::Turn);
    e.set_tool_option(crate::tools::ASK_NAME, OptionValue::Toggle(true));
    e.set_tool(ToolKind::Box);
    assert!(e.tool_options().iter().any(|o| o.key == crate::tools::ASK_NAME && o.value == OptionValue::Toggle(true)));
    assert!(e.tool_options().iter().any(|o| o.key == "offset" && o.value == OptionValue::Length(3.0)));
}

// ---- the undo histories' memory (docs/model/undo.md#memory-cap) ---------------------------

/// An editor where `n` boxes were added then deleted, one after the other: its history holds
/// each alone, in the state before its deletion.
fn editor_with_deleted_boxes(n: usize) -> Editor {
    let mut e = Editor::new(Document::new(), Box::new(skraft_kernel::Inline::default()));
    let root = e.document().root();
    for i in 0..n {
        let primitive = Primitive::Box { origin: DVec3::X * 50.0 * i as f64, size: DVec3::splat(10.0) };
        e.apply(Operation::AddPrimitive { context: root, primitive, placement: glam::DAffine3::IDENTITY, material: None }).unwrap();
        // The part deleted with it (an occurrence erased leaves its part unused, still there).
        let part = e.document().assembly(root).unwrap().occurrences().last().unwrap().component;
        e.apply(Operation::DeleteComponent { component: part }).unwrap();
    }
    e
}

#[test]
fn the_history_weighs_its_meshes_and_forgets_them_with_its_states() {
    let mut e = editor_with_deleted_boxes(3);
    assert_eq!(e.scene().kept_solids(), 3, "the deleted boxes' meshes, kept for undo");
    let held = e.history_bytes();
    let first = e.history().document().clone();
    e.undo();
    let solid = e.document().components().find_map(|c| c.as_part()).unwrap().shape().clone();
    let one_box = e.scene().solid_bytes(&solid);
    assert!(one_box > solid.approx_bytes(), "its mesh counts");
    e.redo();
    assert_eq!(e.document(), &first);
    assert!(held > 3 * one_box && held < 4 * one_box, "{held} bytes held, {one_box} per box");
    // Everything it can: the last step to undo stays, with its box.
    let freed = e.forget_history(u64::MAX);
    assert_eq!(held - freed, e.history_bytes());
    assert_eq!(e.scene().kept_solids(), 1, "the meshes of the boxes forgotten are dropped");
    let last = e.journal().since(0).last().map(|entry| entry.event.clone());
    assert!(matches!(last, Some(journal::Event::Forgotten { steps: 5, bytes }) if bytes == freed), "{last:?}");
    e.undo();
    assert_eq!(e.document().components().count(), 1, "the last box back, with its mesh");
    assert_eq!(e.scene().kept_solids(), 1);
    assert!(!e.history().can_undo());
}

#[test]
fn the_histories_are_capped_together_the_first_ones_first() {
    let (mut a, mut b) = (editor_with_deleted_boxes(2), editor_with_deleted_boxes(2));
    let (held_a, held_b) = (a.history_bytes(), b.history_bytes());
    assert_eq!(cap_histories(&mut [&mut a, &mut b], held_a + held_b), held_a + held_b, "within the budget: nothing forgotten");
    let held = cap_histories(&mut [&mut a, &mut b], held_a + held_b - 1);
    assert_eq!((a.history().forgotten(), b.history().forgotten()), (1, 0), "the first editor's oldest state");
    assert_eq!(held, a.history_bytes() + b.history_bytes());
    assert!(held < held_a + held_b);
    // Nothing more to forget in the first: the second's turn.
    cap_histories(&mut [&mut a, &mut b], 0);
    assert!(!a.history().can_forget() && !b.history().can_forget());
    assert!(a.history().can_undo() && b.history().can_undo());
}

#[test]
fn the_copy_tools_draw_dotted_boxes_but_around_a_box() {
    let mut e = top_editor(Document::new());
    e.set_tool(ToolKind::Cylinder);
    click(&mut e, DVec3::new(100.0, 100.0, 0.0));
    e.input(ToolInput::Vcb("50".into()));
    e.input(ToolInput::Vcb("30".into()));
    let cylinder = drawn(&e);
    e.set_tool(ToolKind::Select);
    e.select(vec![cylinder]);
    // The source's box and the copy's, dotted, in the scene: no front drawn over it (OCL's
    // `shell_on_top` off), the copy's where it goes.
    e.set_tool(ToolKind::CopyLine);
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(600.0, 110.0, 0.0)) });
    let v = e.view();
    let boxes = &v.drawing.boxes;
    assert_eq!(boxes.len(), 2);
    assert!(
        boxes.iter().all(|b| !b.shell_on_top && b.pen.stipple == crate::draw::Stipple::Dotted && b.pen.color == crate::tools::INK.into())
    );
    let x0 = |b: &crate::draw::BoxEdges| b.bounds.frame.transform_point3(b.bounds.lo).x;
    assert!(x0(&boxes[1]) - x0(&boxes[0]) > 100.0, "{} {}", x0(&boxes[0]), x0(&boxes[1]));
    // The measure, a dimension's line: dashed in the scene, translucent where faces hide it
    // (docs/render/drawer.md#dimensions).
    let dims: Vec<_> = v.drawing.lines.iter().filter(|l| l.pen == crate::draw::Pen::dimension(l.pen.color)).collect();
    assert_eq!(dims.len(), 1, "{:?}", v.drawing.lines);
    assert!(dims[0].pen.hidden.is_some_and(|h| h.alpha < 1.0 && h.stipple == dims[0].pen.stipple));

    // A box alone: none; a box with the cylinder: their box is not filled, drawn.
    let board = draw_box(&mut e, DVec3::new(0.0, 300.0, 0.0), DVec3::new(100.0, 400.0, 0.0), "18");
    e.select(vec![board]);
    e.set_tool(ToolKind::CopyLine);
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(600.0, 350.0, 0.0)) });
    assert!(e.view().drawing.boxes.is_empty(), "one box: none");
    e.set_tool(ToolKind::Select);
    e.select(vec![cylinder, board]);
    e.set_tool(ToolKind::CopyLine);
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(600.0, 350.0, 0.0)) });
    assert_eq!(e.view().drawing.boxes.len(), 2, "a cylinder and a box: their box");
}

/// The solid tools gather several targets and tools, as OpenCutList's (docs/tools/solid-tools.md).
#[test]
fn solid_tools_gather_targets_and_tools_with_shift() {
    let mut e = top_editor(Document::new());
    let left = draw_box(&mut e, DVec3::ZERO, DVec3::new(200.0, 200.0, 0.0), "50");
    let right = draw_box(&mut e, DVec3::new(300.0, 0.0, 0.0), DVec3::new(500.0, 200.0, 0.0), "50");
    let root = e.document().root();
    let drill = |e: &mut Editor, x: f64| {
        let op = Operation::AddPrimitive {
            context: root,
            primitive: Primitive::Cylinder { origin: DVec3::new(x, 100.0, -10.0), radius: 10.0, height: 80.0 },
            placement: glam::DAffine3::IDENTITY,
            material: None,
        };
        e.apply(op).unwrap().unwrap().created[0]
    };
    let (d1, d2) = (drill(&mut e, 100.0), drill(&mut e, 400.0));
    let shift = Modifiers { toggle: true, ..Modifiers::default() };
    let shift_click = |e: &mut Editor, p: DVec3| {
        let at = px(e, p);
        e.input(ToolInput::Hover { px: at });
        e.input(ToolInput::Click { px: at, mods: shift });
    };
    let marks = |e: &Editor, m: Mark| e.view().highlights.iter().filter(|h| h.mark == m).count();
    let enter = |e: &mut Editor| e.input(ToolInput::Key { key: Key::Enter, mods: Modifiers::default() });

    e.set_tool(ToolKind::Boolean(Boolean::Difference));
    // « Conserver B » is shared by the three tools.
    e.set_tool_option("keep_tools", OptionValue::Toggle(true));
    e.set_tool(ToolKind::Boolean(Boolean::Union));
    assert!(e.tool_options().iter().any(|o| o.key == "keep_tools" && o.value == OptionValue::Toggle(true)));
    e.set_tool_option("keep_tools", OptionValue::Toggle(false));

    e.set_tool(ToolKind::Boolean(Boolean::Difference));
    shift_click(&mut e, DVec3::new(20.0, 20.0, 50.0));
    // A plain click now adds to the set gathering, Enter goes on.
    click(&mut e, DVec3::new(320.0, 20.0, 50.0));
    assert_eq!(marks(&e, Mark::Picked), 2, "{}", e.status());
    assert!(e.status().starts_with("Sélection multiple"), "{}", e.status());
    enter(&mut e);
    shift_click(&mut e, DVec3::new(100.0, 100.0, 70.0));
    // A target is not a tool.
    click(&mut e, DVec3::new(20.0, 20.0, 50.0));
    assert_eq!(marks(&e, Mark::PickedTool), 1);
    click(&mut e, DVec3::new(400.0, 100.0, 70.0));
    assert_eq!(marks(&e, Mark::PickedTool), 2);
    // Esc lets the tools go, then the targets.
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    assert_eq!((marks(&e, Mark::Picked), marks(&e, Mark::PickedTool)), (2, 0));
    shift_click(&mut e, DVec3::new(100.0, 100.0, 70.0));
    click(&mut e, DVec3::new(400.0, 100.0, 70.0));
    enter(&mut e);
    let doc = e.document();
    assert!(doc.root_assembly().occurrence(d1).is_none() && doc.root_assembly().occurrence(d2).is_none(), "{}", e.status());
    for o in [left, right] {
        assert_eq!(doc.resolve_part(&EntityPath::root(o)).unwrap().1.machinings().len(), 1);
    }
    assert_eq!(marks(&e, Mark::Picked), 0, "back to the targets");

    // A union takes one target: a click goes on to the tools.
    e.set_tool(ToolKind::Boolean(Boolean::Union));
    shift_click(&mut e, DVec3::new(20.0, 20.0, 50.0));
    assert!(e.status().contains("pièce outil B"), "{}", e.status());
}

/// The solid tools' steps bar, and their validation while a set gathers (docs/ui/interface.md#steps-bar).
#[test]
fn solid_tools_tell_their_step_and_wait_for_validation() {
    let mut e = top_editor(Document::new());
    draw_box(&mut e, DVec3::ZERO, DVec3::new(200.0, 200.0, 0.0), "50");
    draw_box(&mut e, DVec3::new(300.0, 0.0, 0.0), DVec3::new(500.0, 200.0, 0.0), "50");
    let shift = Modifiers { toggle: true, ..Modifiers::default() };
    e.set_tool(ToolKind::Boolean(Boolean::Difference));
    let step = |e: &Editor| e.tool_steps().map(|s| (s.count, s.index));
    assert_eq!(step(&e), Some((2, 0)));
    assert!(!e.tool_awaits_validation());
    let at = px(&e, DVec3::new(20.0, 20.0, 50.0));
    e.input(ToolInput::Hover { px: at });
    e.input(ToolInput::Click { px: at, mods: shift });
    assert!(e.tool_awaits_validation(), "a Shift click: the set waits");
    // ✓ is Enter: on to the tools.
    e.input(ToolInput::Key { key: Key::Enter, mods: Modifiers::default() });
    assert_eq!(step(&e), Some((2, 1)));
    assert!(!e.tool_awaits_validation(), "no tool yet");
    // ✗ is Esc: back to A.
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    assert_eq!(step(&e), Some((2, 0)));
    e.set_tool(ToolKind::Move);
    assert_eq!(e.tool_steps(), None);
}

/// The components library (docs/ui/components-library.md): choosing, selecting the occurrences,
/// replacing the selection, deleting, purging.
#[test]
fn library_chooses_selects_replaces_deletes_and_purges() {
    let (mut e, [a, _, c, g, g2]) = drawers_editor();
    let root = e.document().root();
    let component = |e: &Editor, path: EntityPath| e.document().resolve(&path).unwrap().component;
    let (pa, pc, asm) =
        (component(&e, EntityPath::new(vec![g], a)), component(&e, EntityPath::root(c)), component(&e, EntityPath::root(g)));

    // Chosen until anything else is: a selection, a click in the view.
    e.choose_component(pc);
    assert_eq!(e.chosen_component(), Some(pc));
    e.select(vec![c]);
    e.poll();
    assert_eq!(e.chosen_component(), None);
    e.choose_component(pc);
    assert!(e.selection().is_empty(), "choosing lets the selection go");
    click(&mut e, DVec3::new(5000.0, 5000.0, 0.0));
    assert_eq!(e.chosen_component(), None);

    // Its occurrences: the context's children as ids, deeper ones as paths under « Pièces ».
    e.select_occurrences_of(pc);
    assert_eq!(e.selection(), &[c]);
    e.select_occurrences_of(pa);
    assert!(e.pick_parts());
    assert_eq!(e.deep_selection(), &[EntityPath::new(vec![g], a), EntityPath::new(vec![g2], a)]);

    // Replaced: C by A's part, placed as it was.
    e.select(vec![c]);
    let before = e.document().resolve(&EntityPath::root(c)).unwrap().transform;
    assert!(!e.can_replace_selection(pc), "already all of it");
    e.replace_selection(pa);
    assert_eq!(component(&e, EntityPath::root(c)), pa);
    assert_eq!(e.document().resolve(&EntityPath::root(c)).unwrap().transform, before);
    assert!(e.document().component(pc).is_ok() && e.unused_components() == vec![pc], "C's part unused, kept");
    assert_eq!(e.document().counts_in_model()[&pa], 3);
    e.undo();
    assert_eq!(component(&e, EntityPath::root(c)), pc);

    // The assembly edited is neither deleted nor placed into itself.
    e.enter(g);
    assert!(e.on_active_path(asm) && !e.can_delete_component(asm));
    e.select(vec![a]);
    assert!(!e.can_replace_selection(asm));
    e.select_model();
    assert!(e.can_delete_component(asm));

    // Deleted with all its occurrences, at any depth.
    e.delete_component(pa);
    let doc = e.document();
    assert!(doc.component(pa).is_err());
    assert_eq!(doc.assembly(asm).unwrap().occurrences().len(), 1, "B left in each copy");
    e.undo();

    // A part placed nowhere is unused, purged with « Purger les inutilisés ».
    e.apply(Operation::RenameComponent { component: pc, name: "Tiroir".into() }).unwrap();
    e.apply(Operation::Delete { context: root, entities: vec![c] }).unwrap();
    assert_eq!(e.unused_components(), vec![pc]);
    e.purge_unused();
    assert!(e.document().component(pc).is_err() && e.unused_components().is_empty());
    assert!(e.document().component(pa).is_ok());
}

/// Placing from the library (docs/ui/components-library.md#placing): carried by the origin of
/// its axes as a paste, put down by a click (one step), given up by Esc; refused into itself
/// and while a part is open.
#[test]
fn library_places_a_component_carried_by_its_origin() {
    let (mut e, [a, _, c, g, g2]) = drawers_editor();
    let root = e.document().root();
    let pc = e.document().resolve(&EntityPath::root(c)).unwrap().component;
    let asm = e.document().resolve(&EntityPath::root(g)).unwrap().component;
    let count = |e: &Editor| e.document().root_assembly().occurrences().len();
    let before = count(&e);
    let steps = e.history().journal().count();

    assert!(e.place_component(pc));
    assert!(e.carrying() && e.tool_kind() == ToolKind::Move);
    assert_eq!(e.chosen_component(), Some(pc), "chosen while carried");
    // Not drawn until the pointer is over the view; no distance from its origin.
    assert!(e.preview().away && e.preview().ghost.is_none());
    assert!(e.vcb().is_none(), "no distance to type");
    let to = DVec3::new(500.0, 500.0, 0.0);
    e.input(ToolInput::Hover { px: px(&e, to) });
    assert!(!e.preview().away && e.preview().ghost.is_some());
    assert!(e.preview().drawing.is_empty(), "no line nor length from its origin");
    e.leave();
    assert!(e.preview().away, "the pointer gone off the view: not drawn");
    e.input(ToolInput::Hover { px: px(&e, to) });
    e.input(ToolInput::Click { px: px(&e, to), mods: Modifiers::default() });
    assert!(!e.carrying() && e.tool_kind() == ToolKind::Select, "the tool before is back");
    assert_eq!(count(&e), before + 1);
    assert_eq!(e.history().journal().count(), steps + 1, "one step");
    let placed = *e.selection().first().expect("put down, selected");
    let o = e.document().resolve(&EntityPath::root(placed)).unwrap();
    assert_eq!(o.component, pc);
    assert!(close(o.transform.translation, to), "its origin at the cursor: {}", o.transform.translation);

    // Given up: nothing left of it.
    assert!(e.place_component(pc));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(-300.0, 0.0, 0.0)) });
    e.input(ToolInput::Key { key: Key::Escape, mods: Modifiers::default() });
    assert!(!e.carrying());
    assert_eq!(count(&e), before + 1);
    assert_eq!(e.history().journal().count(), steps + 1);

    // Into the assembly edited, through its copy placed 300 further along Y: put down at the
    // cursor in the world, so in the assembly 300 back.
    e.enter(g2);
    assert!(e.place_component(pc));
    let to = DVec3::new(-500.0, -500.0, 0.0);
    e.input(ToolInput::Hover { px: px(&e, to) });
    e.input(ToolInput::Click { px: px(&e, to), mods: Modifiers::default() });
    let placed = *e.selection().first().unwrap();
    let o = e.document().resolve(&EntityPath::new(vec![g2], placed)).unwrap();
    assert!(close(o.transform.translation, DVec3::new(-500.0, -800.0, 0.0)), "{}", o.transform.translation);
    e.select_model();

    // Into itself, or into an open part: refused.
    e.enter(g);
    assert!(!e.can_place_component(asm) && !e.place_component(asm));
    assert!(e.can_place_component(pc));
    e.enter(a);
    assert!(e.library_disabled() && !e.place_component(pc));
    e.select_model();
    assert_eq!(root, e.document().root());
}

#[test]
fn round_tool_picks_edges_previews_and_applies() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(200.0, 100.0, 0.0), "20");
    // Another part beside it.
    draw_box(&mut e, DVec3::new(300.0, 0.0, 0.0), DVec3::new(500.0, 100.0, 0.0), "10");
    let path = EntityPath::root(a);
    let volume = |e: &Editor| e.document().resolve_part(&path).unwrap().1.shape().volume().unwrap();
    let sharp = volume(&e);
    e.select(Vec::new());
    e.set_tool(ToolKind::Round);
    // The top's edge along y = 0, seen from above: picked, then the radius typed.
    click(&mut e, DVec3::new(100.0, 0.0, 20.0));
    // An edge of another part is not taken once one of this one is.
    click(&mut e, DVec3::new(400.0, 0.0, 10.0));
    e.input(ToolInput::Vcb("5".into()));
    let preview = e.preview().blend.expect("a round previewed");
    assert!(matches!(&preview.op, Operation::Blend { size, picks, .. } if *size == 5.0 && picks.len() == 1), "{:?}", preview.op);
    assert!(e.tool_awaits_validation());
    // The kernel's preview: no refusal.
    e.poll();
    assert!(!e.status().contains("mm au plus"), "{}", e.status());
    key(&mut e, Key::Enter);
    e.poll();
    let off = (1.0 - std::f64::consts::FRAC_PI_4) * 25.0 * 200.0;
    assert!((volume(&e) - (sharp - off)).abs() < 1e-2, "{} vs {}: {}", volume(&e), sharp - off, e.status());
    let (_, part, _) = e.document().resolve_part(&path).unwrap();
    assert_eq!(part.def_of(&part.bodies[0]).nature(), skraft_model::Nature::Round);
    assert!(e.preview().blend.is_none(), "the picks gone once applied");
}

#[test]
fn round_tool_picks_a_node_s_edges_again_in_it() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(200.0, 100.0, 0.0), "20");
    let path = EntityPath::root(a);
    let volume = |e: &Editor| e.document().resolve_part(&path).unwrap().1.shape().volume().unwrap();
    let sharp = volume(&e);
    e.select(Vec::new());
    e.set_tool(ToolKind::Round);
    click(&mut e, DVec3::new(100.0, 0.0, 20.0));
    e.input(ToolInput::Vcb("5".into()));
    key(&mut e, Key::Enter);
    e.poll();
    let off = (1.0 - std::f64::consts::FRAC_PI_4) * 25.0;
    assert!((volume(&e) - (sharp - off * 200.0)).abs() < 1e-2);
    // The part opened, its node's edges picked again there: one more (along x = 200).
    e.set_tool(ToolKind::Select);
    e.enter(a);
    let node = e.document().resolve_part(&path).unwrap().1.bodies[0].id;
    e.edit_blend(node);
    assert_eq!(e.open_group(), &[node]);
    assert_eq!(e.tool_kind(), ToolKind::Round);
    // Its edges as they are: nothing to validate yet.
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(100.0, 50.0, 20.0)) });
    assert!(!e.tool_awaits_validation());
    click(&mut e, DVec3::new(200.0, 50.0, 20.0));
    let preview = e.preview().blend.expect("the node changed, previewed");
    assert!(matches!(&preview.op, Operation::SetBlend { picks, size, .. } if picks.len() == 2 && *size == 5.0), "{:?}", preview.op);
    assert!(e.tool_awaits_validation());
    key(&mut e, Key::Enter);
    e.poll();
    // Validated: the node closed and selected, the selection tool back, nothing waiting.
    assert!(e.open_group().is_empty());
    assert_eq!(e.tool_kind(), ToolKind::Select);
    assert_eq!(e.selection(), &[node]);
    assert!(!e.tool_awaits_validation());
    // The two rounds meet at the corner: a little less than both.
    let both = sharp - off * 300.0;
    assert!((volume(&e) - both).abs() < 30.0, "{} vs {both}: {}", volume(&e), e.status());
    let (_, part, _) = e.document().resolve_part(&path).unwrap();
    assert_eq!(part.def_of(&part.bodies[0]).op().unwrap().edges().len(), 2);
}

#[test]
fn a_node_opened_shows_its_children_raw_and_the_bodies_around_it_faded() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(200.0, 100.0, 0.0), "20");
    let root = e.document().root();
    let drill = e
        .apply(Operation::AddPrimitive {
            context: root,
            primitive: Primitive::Cylinder { origin: DVec3::new(50.0, 50.0, -5.0), radius: 5.0, height: 30.0 },
            placement: glam::DAffine3::IDENTITY,
            material: None,
        })
        .unwrap()
        .unwrap()
        .created[0];
    e.apply(Operation::boolean(EntityPath::root(a), EntityPath::root(drill), Boolean::Difference, skraft_model::Scope::All)).unwrap();
    let path = EntityPath::root(a);
    let (part, p, _) = e.document().resolve_part(&path).unwrap();
    let (board, hole) = (p.positives().next().unwrap().id, p.negatives().next().unwrap());
    let (hole, hole_def) = (hole.id, hole.def);
    // The board alone rounded, in the part open: the drill stays beside the node.
    e.select(Vec::new());
    e.enter(a);
    e.set_tool(ToolKind::Round);
    click(&mut e, DVec3::new(100.0, 0.0, 20.0));
    key(&mut e, Key::Enter);
    e.poll();
    let (_, p, _) = e.document().resolve_part(&path).unwrap();
    let node = p.positives().next().unwrap().id;
    assert!(p.def_of(p.body(node).unwrap()).op().is_some() && p.body(hole).is_some() && p.body(board).is_none());
    // The node opened: the part's shape (its result) not drawn; the drill, around it, faded.
    e.set_tool(ToolKind::Select);
    e.enter(node);
    assert_eq!(e.open_group(), &[node]);
    let v = e.view();
    assert!(!v.batches.iter().any(|(def, ps)| *def == part && !ps.is_empty()), "the part's shape hidden");
    let around = v.batches.iter().find(|(def, _)| *def == crate::scene::recipe_key(hole_def)).expect("the drill drawn");
    assert!(around.1.iter().all(|p| p.faded && p.ghost));
    // Under the selection tool, the node said open, by its name, with how its edges change.
    let (open, name, _) = e.open_node().expect("the node open");
    assert_eq!(open, node);
    assert!(!e.blending() && e.status().contains(&name), "{}", e.status());
}

/// A board 200 × 100 × 20 notched at its corner (x ≥ 150, y ≥ 50), the notch's two rims along
/// the top rounded: a node whose edges lie between its two children. The part, the node.
fn notched_round(e: &mut Editor) -> (Id, Id) {
    use skraft_model::{Scope, Sign};
    let a = draw_box(e, DVec3::ZERO, DVec3::new(200.0, 100.0, 0.0), "20");
    let path = EntityPath::root(a);
    let board = e.document().resolve_part(&path).unwrap().1.bodies[0].id;
    let notch = Operation::AddBody {
        target: path.clone(),
        primitive: Primitive::Box { origin: DVec3::new(150.0, 50.0, -5.0), size: DVec3::new(60.0, 60.0, 30.0) },
        placement: glam::DAffine3::IDENTITY,
        sign: Sign::Negative,
        scope: Scope::All,
    };
    let notch = e.apply(notch).unwrap().unwrap().bodies[0];
    let round = Operation::Blend {
        target: path,
        bodies: vec![board, notch],
        blend: skraft_model::kernel::names::Blend::Round,
        size: 3.0,
        picks: vec![DVec3::new(150.0, 75.0, 20.0), DVec3::new(175.0, 50.0, 20.0)],
        scope: Scope::All,
    };
    let node = e.apply(round).unwrap().unwrap().bodies[0];
    e.poll();
    (a, node)
}

/// The edges of the node open, as they are lit.
fn lit_edges(e: &Editor) -> Vec<crate::infer::EdgeHit> {
    let (_, _, op) = e.open_node().expect("a node open");
    tools::BlendTool::load(&e.cx(), &op)
}

#[test]
fn a_node_opened_lights_its_edges_between_two_children() {
    let mut e = top_editor(Document::new());
    let (a, node) = notched_round(&mut e);
    e.enter(a);
    e.enter(node);
    e.poll();
    let lit = lit_edges(&e);
    assert_eq!(lit.len(), 2);
    for l in &lit {
        assert!((l.middle - DVec3::new(150.0, 75.0, 20.0)).length() < 1e-6 || (l.middle - DVec3::new(175.0, 50.0, 20.0)).length() < 1e-6);
    }
}

#[test]
fn a_node_read_from_a_file_lights_its_edges_once_its_children_are_computed() {
    let mut e = top_editor(Document::new());
    let (a, node) = notched_round(&mut e);
    // Its children's result is not in the file.
    let read = Document::from_json(&e.document().to_json().unwrap()).unwrap();
    let mut e = top_editor(read);
    e.enter(a);
    e.enter(node);
    assert!(lit_edges(&e).is_empty(), "not computed yet");
    assert!(e.meshing(), "waited for");
    while e.meshing() {
        e.poll();
    }
    assert_eq!(lit_edges(&e).len(), 2);
}

#[test]
fn choosing_the_edges_of_a_node_already_open_starts_its_tool() {
    let mut e = top_editor(Document::new());
    let (a, node) = notched_round(&mut e);
    e.enter(a);
    e.enter(node);
    e.poll();
    assert_eq!(e.tool_kind(), ToolKind::Select);
    e.edit_blend(node);
    assert_eq!(e.tool_kind(), ToolKind::Round);
    assert_eq!(e.open_group(), &[node], "it stays open");
    // Its two edges picked: one more makes three.
    click(&mut e, DVec3::new(100.0, 0.0, 20.0));
    let preview = e.preview().blend.expect("the node changed, previewed");
    assert!(matches!(&preview.op, Operation::SetBlend { picks, .. } if picks.len() == 3), "{:?}", preview.op);
}

#[test]
fn the_round_tool_on_a_node_shows_and_keeps_the_node_s_size() {
    use skraft_model::kernel::names::Blend;
    let size = |e: &Editor| match e.tool_options().into_iter().find(|o| o.key == "size").unwrap().value {
        crate::tools::OptionValue::Length(v) => v,
        v => panic!("{v:?}"),
    };
    let node_size = |e: &Editor, path: &EntityPath, node: Id| {
        let (_, p, _) = e.document().resolve_part(path).unwrap();
        p.def_of(p.body(node).unwrap()).op().unwrap().blend().unwrap().size
    };
    let mut e = top_editor(Document::new());
    let (a, node) = notched_round(&mut e);
    let path = EntityPath::root(a);
    let set = |size: f64| Operation::SetBlend {
        target: path.clone(),
        body: node,
        blend: Blend::Round,
        size,
        picks: Vec::new(),
        scope: skraft_model::Scope::All,
    };
    e.set_tool(ToolKind::Round);
    let saved = size(&e);
    e.set_tool(ToolKind::Select);
    e.enter(a);
    // Its radius changed in the entity info, then its tool started: it shows that one.
    e.apply(set(7.0)).unwrap();
    e.poll();
    e.edit_blend(node);
    assert_eq!(size(&e), 7.0);
    // Changed again in the entity info, the tool on: followed, and not put back by a validation.
    e.apply(set(6.0)).unwrap();
    e.poll();
    assert_eq!(size(&e), 6.0);
    click(&mut e, DVec3::new(100.0, 0.0, 20.0));
    assert!(matches!(e.preview().blend.unwrap().op, Operation::SetBlend { size, .. } if size == 6.0));
    // Typed in the bar: the node's to validate, not the tool's saved size.
    e.set_tool_option("size", crate::tools::OptionValue::Length(5.0));
    assert_eq!(size(&e), 5.0);
    assert!(e.tool_awaits_validation());
    key(&mut e, Key::Enter);
    e.poll();
    assert_eq!(node_size(&e, &path, node), 5.0, "{}", e.status());
    e.set_tool(ToolKind::Round);
    assert_eq!(size(&e), saved, "the tool's own size untouched");
}

#[test]
fn a_node_keeps_its_edges_on_a_group_of_flush_cutters_when_a_child_is_stretched() {
    use skraft_model::kernel::names::Blend;
    use skraft_model::{Scope, Sign};
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(200.0, 100.0, 0.0), "20");
    let path = EntityPath::root(a);
    let board = e.document().resolve_part(&path).unwrap().1.bodies[0].id;
    // Two notches through the board, along its end, at its two long sides.
    let cutter = |e: &mut Editor, y: f64| {
        let op = Operation::AddBody {
            target: path.clone(),
            primitive: Primitive::Box { origin: DVec3::new(100.0, y, 0.0), size: DVec3::new(100.0, 30.0, 20.0) },
            placement: glam::DAffine3::IDENTITY,
            sign: Sign::Negative,
            scope: Scope::All,
        };
        e.apply(op).unwrap().unwrap().bodies[0]
    };
    let (back, front) = (cutter(&mut e, 70.0), cutter(&mut e, 0.0));
    e.enter(a);
    e.select(vec![back, front]);
    e.group_bodies();
    let group = e.selection()[0];
    e.poll();
    let round = Operation::Blend {
        target: path.clone(),
        bodies: vec![board, group],
        blend: Blend::Round,
        size: 3.0,
        picks: vec![DVec3::new(150.0, 70.0, 20.0), DVec3::new(150.0, 30.0, 20.0)],
        scope: Scope::All,
    };
    let node = e.apply(round).unwrap().unwrap().bodies[0];
    e.poll();
    // Each edge named by the board's top and a wall of its own cutter.
    let (_, p, _) = e.document().resolve_part(&path).unwrap();
    let edges = p.def_of(p.body(node).unwrap()).op().unwrap().edges().to_vec();
    assert_eq!(edges.len(), 2);
    for x in &edges {
        assert!(x.name.faces.iter().any(|t| t.path.first() == Some(&group.0)), "{:?}", x.name);
    }
    // The board made longer: they are still there.
    let stretch = Operation::StretchBodies {
        target: path.clone().in_group(vec![node]),
        bodies: vec![board],
        frame: glam::DAffine3::IDENTITY,
        axis: DVec3::X,
        cuts: vec![50.0],
        moves: vec![-30.0, 0.0],
        scope: Scope::All,
    };
    e.apply(stretch).unwrap();
    e.poll();
    let (_, p, _) = e.document().resolve_part(&path).unwrap();
    assert_eq!(p.def_of(p.body(node).unwrap()).group.as_ref().unwrap().refused, None);
}

#[test]
fn round_tool_keeps_a_node_s_edges_between_children_when_one_is_added() {
    let mut e = top_editor(Document::new());
    let (a, node) = notched_round(&mut e);
    let path = EntityPath::root(a);
    e.set_tool(ToolKind::Select);
    e.enter(a);
    e.edit_blend(node);
    e.poll();
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(100.0, 50.0, 20.0)) });
    assert!(!e.tool_awaits_validation(), "its edges as they are");
    // One more, along y = 0: the two of the notch stay.
    click(&mut e, DVec3::new(100.0, 0.0, 20.0));
    let preview = e.preview().blend.expect("the node changed, previewed");
    assert!(matches!(&preview.op, Operation::SetBlend { picks, .. } if picks.len() == 3), "{:?}", preview.op);
    key(&mut e, Key::Enter);
    e.poll();
    let (_, part, _) = e.document().resolve_part(&path).unwrap();
    assert_eq!(part.def_of(&part.bodies[0]).op().unwrap().edges().len(), 3, "{}", e.status());
}

#[test]
fn a_node_shows_no_machining_of_its_children_over_its_result() {
    use skraft_model::{Scope, Sign};
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(200.0, 100.0, 0.0), "20");
    let path = EntityPath::root(a);
    let board = e.document().resolve_part(&path).unwrap().1.bodies[0].id;
    let drill = |e: &mut Editor, x: f64| {
        let op = Operation::AddBody {
            target: path.clone(),
            primitive: Primitive::Cylinder { origin: DVec3::new(x, 50.0, -5.0), radius: 4.0, height: 30.0 },
            placement: glam::DAffine3::IDENTITY,
            sign: Sign::Negative,
            scope: Scope::All,
        };
        e.apply(op).unwrap().unwrap().bodies[0]
    };
    let inside = drill(&mut e, 50.0);
    let beside = drill(&mut e, 150.0);
    let round = Operation::Blend {
        target: path.clone(),
        bodies: vec![board, inside],
        blend: skraft_model::kernel::names::Blend::Round,
        size: 3.0,
        picks: vec![DVec3::new(100.0, 0.0, 20.0)],
        scope: Scope::All,
    };
    let node = e.apply(round).unwrap().unwrap().bodies[0];
    e.enter(a);
    // The node at the part's first level: its drill not drawn over its result.
    assert!(e.scene().recipe().unwrap().nested.is_empty());
    // In a plain group, beside another drill: that one shows, the node's still does not.
    e.select(vec![node, beside]);
    e.group_bodies();
    let group = e.selection()[0];
    let r = e.scene().recipe().unwrap();
    assert_eq!(r.nested.len(), 1);
    assert_eq!(r.nested[0].body, group);
    assert!((r.nested[0].placement.translation - DVec3::new(150.0, 50.0, -5.0)).length() < 1e-9);
}

#[test]
fn move_out_takes_a_body_one_level_up_and_out_of_the_part() {
    // A board 200 × 100 × 20, a drill in it, the two rounded on the top's front edge.
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(200.0, 100.0, 0.0), "20");
    let path = EntityPath::root(a);
    let board = e.document().resolve_part(&path).unwrap().1.bodies[0].id;
    let drill = e
        .apply(Operation::AddBody {
            target: path.clone(),
            primitive: Primitive::Cylinder { origin: DVec3::new(50.0, 50.0, -5.0), radius: 4.0, height: 30.0 },
            placement: glam::DAffine3::IDENTITY,
            sign: skraft_model::Sign::Negative,
            scope: skraft_model::Scope::All,
        })
        .unwrap()
        .unwrap()
        .bodies[0];
    let round = Operation::Blend {
        target: path.clone(),
        bodies: vec![board, drill],
        blend: skraft_model::kernel::names::Blend::Round,
        size: 3.0,
        picks: vec![DVec3::new(100.0, 0.0, 20.0)],
        scope: skraft_model::Scope::All,
    };
    let node = e.apply(round).unwrap().unwrap().bodies[0];
    // The node open, its drill selected: « Sortir du groupe ».
    e.enter(a);
    e.enter(node);
    assert_eq!(e.open_group(), &[node]);
    e.select(vec![drill]);
    assert!(e.can_move_out());
    e.move_out();
    e.poll();
    assert!(e.open_group().is_empty(), "the node left");
    assert_eq!(e.selection(), &[drill], "the drill selected where it went");
    let (_, p, _) = e.document().resolve_part(&path).unwrap();
    assert!(p.bodies.iter().any(|b| b.id == drill));
    // Again, at the part's root: out of the part, a part beside it, selected.
    e.move_out();
    e.poll();
    assert_eq!(e.open_part(), None);
    assert_eq!(e.document().assembly(e.document().root()).unwrap().occurrences().len(), 2);
    assert_eq!(e.selection().len(), 1);
    assert_ne!(e.selection()[0], a);
}

#[test]
fn move_starts_from_a_corner_of_the_box_of_several_parts() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "50");
    let b = draw_box(&mut e, DVec3::new(300.0, 300.0, 0.0), DVec3::new(400.0, 400.0, 0.0), "50");
    e.select(vec![a, b]);
    e.set_tool(ToolKind::Move);
    // A corner of their common box, in the air: on neither part.
    let corner = DVec3::new(0.0, 400.0, 50.0);
    e.input(ToolInput::Hover { px: px(&e, corner) });
    let (at, kind) = e.inference().expect("inferred");
    assert_eq!(kind, InferKind::BoxCorner);
    assert!(close(DVec3::new(at.x, at.y, 50.0), corner), "{at}");
    click(&mut e, corner);
    e.input(ToolInput::Hover { px: px(&e, corner + DVec3::new(50.0, 0.0, 0.0)) });
    e.input(ToolInput::Vcb("50".into()));
    let (lo, _) = bounds_of(&e, &EntityPath::root(a));
    assert!(close(lo, DVec3::new(50.0, 0.0, 0.0)), "{lo}");
}

#[test]
fn a_grip_of_the_box_turns_about_the_face_centre_in_move() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(800.0, 400.0, 0.0), "50");
    e.set_tool(ToolKind::Move);
    // The grip towards +X of the top face, 28 px inside its side.
    let center = DVec3::new(400.0, 200.0, 50.0);
    let inset = e.camera.world_per_pixel(e.viewport_size(), center) * 28.0;
    let grip = center + DVec3::X * (400.0 - inset);
    let plain = e.preview().drawing.lines.len();
    e.input(ToolInput::Hover { px: px(&e, grip) });
    assert!(e.preview().drawing.lines.len() > plain + 40, "the grips' arrows drawn");
    assert!(e.preview().tip.is_some(), "aimed at, on its point");
    // Along its arrow, off its point: still the grip, its protractor shown.
    let wpp = e.camera.world_per_pixel(e.viewport_size(), grip);
    let on_arrow = DVec3::new(grip.x - 20.0 * wpp * (1.0 - 0.8f64.cos()), grip.y + 20.0 * wpp * 0.8f64.sin(), grip.z);
    e.input(ToolInput::Hover { px: px(&e, on_arrow) });
    assert!(e.preview().tip.is_some(), "aimed at, along its arrow");
    let off = DVec3::new(on_arrow.x - 14.0 * wpp, on_arrow.y + 14.0 * wpp, on_arrow.z);
    e.input(ToolInput::Hover { px: px(&e, off) });
    assert!(e.preview().tip.is_none(), "not aimed at off its arrow");
    click(&mut e, grip);
    assert!(e.vcb().is_some_and(|v| v.1 == "0°"), "straight at the angle");
    e.input(ToolInput::Vcb("90".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(a));
    assert!(close(lo, DVec3::new(200.0, -200.0, 0.0)) && close(hi, DVec3::new(600.0, 600.0, 50.0)), "{lo} {hi}");
    assert_eq!(e.tool_kind(), ToolKind::Move, "the tool stays");
}

#[test]
fn rotate_takes_the_centre_of_a_face_of_the_box() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(200.0, 100.0, 0.0), "50");
    e.set_tool(ToolKind::Rotate);
    let center = DVec3::new(100.0, 50.0, 50.0);
    e.input(ToolInput::Hover { px: px(&e, center) });
    assert_eq!(e.inference().map(|i| i.1), Some(InferKind::Center));
    click(&mut e, center);
    click(&mut e, DVec3::new(190.0, 50.0, 50.0));
    e.input(ToolInput::Vcb("90".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(a));
    assert!(close(lo, DVec3::new(50.0, -50.0, 0.0)) && close(hi, DVec3::new(150.0, 150.0, 50.0)), "{lo} {hi}");
}

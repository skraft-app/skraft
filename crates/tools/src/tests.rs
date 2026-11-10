//! Whole scenarios driven like the app does: pixels in, operations out.

use glam::{DVec2, DVec3};
use solidsketch_model::{Boolean, Document, EntityPath, Id, Operation, Primitive};

use crate::*;

/// An editor looking straight down (parallel), framing ±1 m.
fn top_editor(doc: Document) -> Editor {
    let mut e = Editor::new(doc, Box::new(solidsketch_kernel::Inline::default()));
    e.set_viewport_size(DVec2::new(1000.0, 1000.0));
    e.set_parallel(true);
    e.camera.set_standard_view(StandardView::Top, Some((DVec3::splat(-1000.0), DVec3::splat(1000.0))), crate::camera::ViewArea::full(e.viewport_size()));
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

/// Draws a box with the tool: two corners clicked, the height typed.
fn draw_box(e: &mut Editor, a: DVec3, b: DVec3, h: &str) -> Id {
    e.set_tool(ToolKind::Box);
    click(e, a);
    click(e, b);
    e.input(ToolInput::Vcb(h.into()));
    e.set_tool(ToolKind::Select);
    assert_eq!(e.selection().len(), 1, "{}", e.status());
    e.selection()[0]
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
    let id2 = e.selection()[0];
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
    let id = e.selection()[0];
    let (lo, hi) = bounds_of(&e, &EntityPath::root(id));
    assert!(close(lo, DVec3::ZERO) && close(hi, DVec3::new(600.0, 600.0, 100.0)), "{lo} {hi}");

    // `;@*2`: the width kept, the length doubled — signed as the drag, here towards -y.
    e.set_tool(ToolKind::Box);
    click(&mut e, DVec3::new(-500.0, -500.0, 0.0));
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(-400.0, -550.0, 0.0)) });
    e.input(ToolInput::Vcb(";@*2".into()));
    e.input(ToolInput::Vcb("10".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(e.selection()[0]));
    assert!(close(lo, DVec3::new(-500.0, -600.0, 0.0)) && close(hi, DVec3::new(-400.0, -500.0, 10.0)), "{lo} {hi}");

    // The height with the base: done at once (`50==`: a cube), and `r;h` for a cylinder.
    e.set_tool(ToolKind::Box);
    click(&mut e, DVec3::new(700.0, 700.0, 0.0));
    e.input(ToolInput::Vcb("50==".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(e.selection()[0]));
    assert!(close(lo, DVec3::new(700.0, 700.0, 0.0)) && close(hi, DVec3::new(750.0, 750.0, 50.0)), "{lo} {hi}");
    e.set_tool(ToolKind::Cylinder);
    click(&mut e, DVec3::new(-700.0, 700.0, 0.0));
    e.input(ToolInput::Vcb("20;-30".into()));
    let (lo, hi) = bounds_of(&e, &EntityPath::root(e.selection()[0]));
    assert!((lo.z + 30.0).abs() < 1e-6 && hi.z.abs() < 1e-6, "{lo} {hi}");

    // A typed point: absolute, then relative to the start.
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
    let id = e.selection()[0];
    let (lo, hi) = bounds_of(&e, &EntityPath::root(id));
    assert!((lo.z + 30.0).abs() < 1e-6 && hi.z.abs() < 1e-6, "{lo} {hi}");
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
    let top = e.selection().iter().map(|id| bounds_of(&e, &EntityPath::root(*id)).0.z).fold(0.0f64, |m, z| if z.abs() > m.abs() { z } else { m });
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
    // Double-click in the void leaves, the group selected.
    e.input(ToolInput::DoubleClick { px: px(&e, DVec3::new(-800.0, -800.0, 0.0)), mods: Modifiers::default() });
    assert!(e.context().is_empty());
    assert_eq!(e.selection(), &[g]);
    // Undo back past the group: the context and selection stay valid.
    e.enter(g);
    e.undo();
    e.undo();
    assert!(e.context().is_empty());
}

#[test]
fn a_selected_assembly_shows_its_content_box() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "50");
    let b = draw_box(&mut e, DVec3::new(300.0, 0.0, 0.0), DVec3::new(400.0, 200.0, 0.0), "80");
    // A part alone: no box.
    e.select(vec![a]);
    assert!(e.view().selection_boxes.is_empty());
    e.select(vec![a, b]);
    e.make_assembly();
    let segs = e.view().selection_boxes;
    assert!(!segs.is_empty());
    // Dashed, the box spans the content's bounds exactly.
    let (lo, hi) = segs.iter().flat_map(|(a, b)| [*a, *b]).fold((DVec3::MAX, DVec3::MIN), |(l, h), p| (l.min(p), h.max(p)));
    assert!(close(lo, DVec3::ZERO) && close(hi, DVec3::new(400.0, 200.0, 80.0)), "{lo} {hi}");
    e.select(vec![]);
    assert!(e.view().selection_boxes.is_empty());
    assert!(e.view().context_box.is_empty());
    // Inside the assembly: its box stays, a little larger than the content.
    let g = { e.select_all(); e.selection()[0] };
    e.enter(g);
    let v = e.view();
    assert!(v.selection_boxes.is_empty());
    let (lo, hi) = v.context_box.iter().flat_map(|(a, b)| [*a, *b]).fold((DVec3::MAX, DVec3::MIN), |(l, h), p| (l.min(p), h.max(p)));
    let d = lo - DVec3::ZERO;
    assert!(d.x < 0.0 && (d.x - d.y).abs() < 1e-3 && (d.x - d.z).abs() < 1e-3, "{lo}");
    assert!(close(hi - DVec3::new(400.0, 200.0, 80.0), -d), "{lo} {hi}");
}

#[test]
fn component_instances_and_scene_caches() {
    let doc = Document::new();
    let root = doc.root();
    let (d, out) = doc.apply(&Operation::AddPrimitive { context: root, primitive: Primitive::Box { origin: DVec3::ZERO, size: DVec3::splat(100.0) }, placement: glam::DAffine3::IDENTITY }).unwrap();
    let (d, out) = d
        .apply(&Operation::MakeAssembly { context: root, entities: out.created, name: None })
        .unwrap();
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

use deferred::Deferred;
use crate::tools::Command;

fn deferred_editor(doc: Document) -> (Editor, Deferred) {
    let k = Deferred::default();
    let mut e = Editor::new(doc, Box::new(k.clone()));
    e.set_viewport_size(DVec2::new(1000.0, 1000.0));
    e.set_parallel(true);
    e.camera.set_standard_view(StandardView::Top, Some((DVec3::splat(-1000.0), DVec3::splat(1000.0))), crate::camera::ViewArea::full(e.viewport_size()));
    (e, k)
}

fn add_cube(e: &mut Editor, k: &Deferred) -> Id {
    let root = e.document().root();
    let op = Operation::AddPrimitive { context: root, primitive: Primitive::Box { origin: DVec3::ZERO, size: DVec3::splat(100.0) }, placement: glam::DAffine3::IDENTITY };
    assert_eq!(e.apply(op), Ok(None), "waits for the kernel");
    k.run_all();
    assert!(e.poll());
    e.document().root_assembly().occurrences[0].id
}

#[test]
fn pending_operation_blocks_editing_until_its_result() {
    let (mut e, k) = deferred_editor(Document::new());
    let root = e.document().root();
    let op = Operation::AddPrimitive { context: root, primitive: Primitive::Box { origin: DVec3::ZERO, size: DVec3::splat(100.0) }, placement: glam::DAffine3::IDENTITY };
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
    let op = Operation::AddPrimitive { context: root, primitive: Primitive::Box { origin: DVec3::ZERO, size: DVec3::splat(100.0) }, placement: glam::DAffine3::IDENTITY };
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
    e.run(Command::Replace { previous: copy.clone(), op: Operation::Array { context: root, entities: vec![s], step: t, count: 5 } });
    assert!(!e.busy());
    assert_eq!(e.document().root_assembly().occurrences.len(), 6);
    assert_eq!(k.queued(), 0, "nothing to mesh: one part");
    e.undo();
    assert_eq!(e.document().root_assembly().occurrences.len(), 1);
    e.redo();

    // A refused array leaves the copy as it was.
    e.undo();
    e.apply(copy.clone()).unwrap();
    e.run(Command::Replace { previous: copy.clone(), op: Operation::Array { context: root, entities: vec![s], step: t, count: 0 } });
    assert_eq!(e.history().undo_op(), Some(&copy));
    assert_eq!(e.document().root_assembly().occurrences.len(), 2);
}

#[test]
fn opened_solids_are_outlined_until_meshed_and_undo_does_not_remesh() {
    let doc = Document::new();
    let root = doc.root();
    let (doc, _) = doc
        .apply(&Operation::AddPrimitive { context: root, primitive: Primitive::Box { origin: DVec3::ZERO, size: DVec3::splat(100.0) }, placement: glam::DAffine3::IDENTITY })
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
    e.view().batches.iter().flat_map(|(def, ps)| {
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

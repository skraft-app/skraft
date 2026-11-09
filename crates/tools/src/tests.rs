//! Whole scenarios driven like the app does: pixels in, operations out.

use glam::{DVec2, DVec3};
use solidsketch_model::{Boolean, DefinitionKind, Document, Entity, EntityPath, Id, Operation, Primitive};

use crate::*;

/// An editor looking straight down (parallel), framing ±1 m.
fn top_editor(doc: Document) -> Editor {
    let mut e = Editor::new(doc);
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

fn bounds_of(e: &Editor, path: &EntityPath) -> (DVec3, DVec3) {
    let s = e.document().resolve(path).unwrap().as_solid().unwrap().solid.clone();
    let (lo, hi) = s.vertex_bounds().unwrap();
    let t = e.document().transform_of(&path.instances).unwrap();
    (t.transform_point3(lo), t.transform_point3(hi))
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

#[test]
fn move_with_axis_lock_and_typed_distance_then_copy() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "50");
    click(&mut e, DVec3::new(50.0, 50.0, 50.0));
    e.set_tool(ToolKind::Move);
    click(&mut e, DVec3::new(50.0, 50.0, 50.0));
    e.input(ToolInput::Key { key: Key::Right, mods: Modifiers::default() });
    e.input(ToolInput::Hover { px: px(&e, DVec3::new(300.0, 80.0, 50.0)) });
    assert!(e.view().highlights.iter().any(|h| h.mark == Mark::Ghost));
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
    assert_eq!(e.document().root_definition().entities.len(), 2);
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
    assert_eq!(e.document().root_definition().entities.len(), 4);
    assert_eq!(e.selection().len(), 3);
    let top = e.selection().iter().map(|id| bounds_of(&e, &EntityPath::root(*id)).0.z).fold(0.0f64, |m, z| if z.abs() > m.abs() { z } else { m });
    assert!((top - 3.0 * dz).abs() < 1e-6, "{top}");
    e.input(ToolInput::Vcb("/4".into()));
    assert_eq!(e.document().root_definition().entities.len(), 5);
    let mut zs: Vec<f64> = e.selection().iter().map(|id| bounds_of(&e, &EntityPath::root(*id)).0.z.abs()).collect();
    zs.sort_by(f64::total_cmp);
    assert!(zs.iter().zip([25.0, 50.0, 75.0, 100.0]).all(|(z, w)| (z - w).abs() < 1e-6), "{zs:?}");
    assert_eq!(e.history().journal().count(), steps);
    e.undo();
    assert_eq!(e.document().root_definition().entities.len(), 1);
}

#[test]
fn groups_enter_fade_exit() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(100.0, 100.0, 0.0), "50");
    let _b = draw_box(&mut e, DVec3::new(300.0, 0.0, 0.0), DVec3::new(400.0, 100.0, 0.0), "50");
    e.select(vec![a]);
    e.make_container(DefinitionKind::Group);
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
    assert_eq!(e.document().definition(gdef).unwrap().entities.len(), 2);
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
fn component_instances_and_scene_caches() {
    let doc = Document::new();
    let root = doc.root();
    let (d, out) = doc.apply(&Operation::AddPrimitive { context: root, primitive: Primitive::Box { origin: DVec3::ZERO, size: DVec3::splat(100.0) }, placement: glam::DAffine3::IDENTITY }).unwrap();
    let (d, out) = d
        .apply(&Operation::MakeContainer { context: root, entities: out.created, kind: DefinitionKind::Component, name: None })
        .unwrap();
    let i1 = out.created[0];
    let def = d.resolve(&EntityPath::root(i1)).unwrap().as_instance().unwrap().definition;
    let t = glam::DAffine3::from_translation(DVec3::new(500.0, 0.0, 0.0));
    let (d, out) = d.apply(&Operation::PlaceInstance { context: root, definition: def, transform: t }).unwrap();
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

    // Make unique: the copy shares the solid's Arc, so its mesh is not recomputed either.
    e.select(vec![i2]);
    e.make_unique();
    assert_eq!(e.scene().definition_meshes().count(), 2);
    for dm in e.scene().definition_meshes() {
        assert!(std::sync::Arc::ptr_eq(&mesh, &dm.parts[0].mesh));
    }
}

#[test]
fn boolean_tool_across_contexts() {
    let mut e = top_editor(Document::new());
    let a = draw_box(&mut e, DVec3::ZERO, DVec3::new(200.0, 200.0, 0.0), "100");
    e.select(vec![a]);
    e.make_container(DefinitionKind::Group);
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
        .created[0];

    e.set_tool(ToolKind::Boolean(Boolean::Difference));
    click(&mut e, DVec3::new(20.0, 20.0, 100.0)); // the box (target), away from the cylinder
    assert_eq!(e.view().highlights.iter().filter(|h| h.mark == Mark::Picked).count(), 1);
    click(&mut e, DVec3::new(100.0, 100.0, 130.0)); // the cylinder's top (tool)
    let doc = e.document();
    assert!(doc.root_definition().entity(cyl).is_none(), "tool consumed: {}", e.status());
    let s = doc.resolve(&EntityPath::new(vec![g], a)).unwrap();
    let Entity::Solid(s) = s else { panic!() };
    assert!(s.solid.topology_counts().0 >= 7);
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
    assert_eq!(e.document().root_definition().entities.len(), 2);

    // `/2` right after: two copies spread over the angle (the last one where the copy was).
    let copy = e.selection()[0];
    let before = bounds_of(&e, &EntityPath::root(copy));
    e.input(ToolInput::Vcb("/2".into()));
    assert_eq!(e.document().root_definition().entities.len(), 3);
    let last = *e.selection().last().unwrap();
    let after = bounds_of(&e, &EntityPath::root(last));
    assert!(close(before.0, after.0) && close(before.1, after.1), "{before:?} {after:?}");
}

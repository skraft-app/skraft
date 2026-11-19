//! Trial for docs/decisions/0042-body-nodes-operators.md: can SolidCore's attribute propagation
//! name a group's result faces by the faces of its leaves, so an edge of the result is named by
//! a pair of (body, face) tags that survives an edit of the group?
//!
//! Each leaf's faces are tagged (`body:+Z`, `hole:wall`, …) before the booleans, the booleans
//! run in one kernel session as `recipe_session` runs them, and the tags are read back on the
//! result's faces; an edge's name is the pair of its two faces' tags. Run with
//! `cargo test --workspace provenance_trial -- --nocapture --test-threads 1` to see the tables.

use std::collections::{BTreeMap, BTreeSet};

use glam::DVec3;
use solidcore::boolean::api_brep as b;
use solidcore::geom::attribute::{
    ApplicationValue, Attribute, AttributeHandlers, AttributeValue, AttributeView, Behavior, ObjectKey, Placement, ids,
};
use solidcore::{Brep, EntityRef, Kernel};
use solidcore_record::brep::{Brep as Record, Loop, Surface};

use crate::Solid;
use crate::engine::kernel;
use crate::profile::Corner;

const ID: u64 = ids::USER_1 + 42;

/// The leaf faces a result face comes from: one normally, several where faces merged (two
/// coplanar tops) or one lay on another.
#[derive(Clone, Debug, PartialEq)]
struct Tag(BTreeSet<String>);

fn tag_on(view: &AttributeView<'_>, x: ObjectKey) -> Option<(solidcore::geom::attribute::AttributeHandle, Tag)> {
    let (h, a) = view.find_attribute(x, ID)?;
    a.application_value::<Tag>().map(|t| (h, t.clone()))
}

/// `x` given `tag`, joined to what it carries.
fn join_onto(view: &AttributeView<'_>, x: ObjectKey, tag: &Tag) -> Vec<Placement> {
    match tag_on(view, x) {
        Some((handle, now)) => {
            let joined = Tag(now.0.union(&tag.0).cloned().collect());
            if joined == now {
                return Vec::new();
            }
            vec![Placement::Edit { handle, value: AttributeValue::Application(ApplicationValue::new(joined)) }]
        }
        None => vec![Placement::Add { on: x, attribute: Attribute::application(ID, Behavior::Copy, tag.clone()) }],
    }
}

impl AttributeHandlers for Tag {
    // Split: the default, each piece keeps the tag.

    /// The store calls one merge for both sources: each is read from the view and joined.
    fn merge(&self, view: &AttributeView<'_>, a: ObjectKey, b: ObjectKey, merged: ObjectKey) -> solidcore::Result<Vec<Placement>> {
        let mut all = BTreeSet::new();
        for x in [a, b, merged] {
            if let Some((_, t)) = tag_on(view, x) {
                all.extend(t.0);
            }
        }
        all.extend(self.0.iter().cloned());
        Ok(join_onto(view, merged, &Tag(all)))
    }

    fn coincident(&self, view: &AttributeView<'_>, _from: ObjectKey, to: ObjectKey) -> solidcore::Result<Vec<Placement>> {
        Ok(join_onto(view, to, self))
    }
}

/// A leaf face's name in its own axes: a plane by the side of the leaf's box it lies on
/// (`+X`…), anything else by its kind and record index.
fn face_name(r: &Record, i: usize, lo: DVec3, hi: DVec3) -> String {
    match &r.faces[i].surface {
        Surface::Plane { origin, u_axis, v_axis } => {
            let n = DVec3::from_array(*u_axis).cross(DVec3::from_array(*v_axis)).normalize();
            let o = DVec3::from_array(*origin);
            let k = (0..3).max_by(|&a, &b| n[a].abs().total_cmp(&n[b].abs())).unwrap();
            let axis = ["X", "Y", "Z"][k];
            if (o[k] - hi[k]).abs() < 1e-9 {
                format!("+{axis}")
            } else if (o[k] - lo[k]).abs() < 1e-9 {
                format!("-{axis}")
            } else {
                format!("plane#{i}")
            }
        }
        Surface::Cylinder { .. } => "wall".into(),
        s => format!("{}#{i}", format!("{s:?}").split([' ', '{', '(']).next().unwrap_or("?").to_lowercase()),
    }
}

/// A profiled leaf's face named as its recipe would: the caps `top` / `bottom`, a straight
/// side by the id of the profile side it lies on, a fillet's face by the ids of the two sides it
/// joins. The trial finds which side a face comes from geometrically (an oracle standing in for
/// the extrusion's own face order); `sides[k]` is the side from corner k to corner k + 1.
fn profile_face_name(r: &Record, i: usize, sides: &[(&str, Corner)], height: f64) -> String {
    let n = sides.len();
    let at = |k: usize| sides[k % n].1.p.extend(0.0);
    match &r.faces[i].surface {
        Surface::Plane { origin, u_axis, v_axis } => {
            let normal = DVec3::from_array(*u_axis).cross(DVec3::from_array(*v_axis)).normalize();
            let o = DVec3::from_array(*origin);
            if normal.z.abs() > 0.99 {
                return if (o.z - height).abs() < 1e-6 { "top".into() } else { "bottom".into() };
            }
            let on = |p: DVec3| (p - o).dot(normal).abs() < 1e-6;
            match (0..n).find(|&k| on(at(k)) && on(at(k + 1))) {
                Some(k) => format!("side:{}", sides[k].0),
                None => format!("plane#{i}"),
            }
        }
        // A fillet's face: SolidCore extrudes the arc as a rational spline, not a cylinder. The
        // filleted corner nearest the face's vertices: it joins the side ending there and the
        // side leaving it.
        _ => {
            let mut ps = Vec::new();
            for l in &r.faces[i].loops {
                if let Loop::Edges { edges } = l {
                    for e in edges {
                        ps.extend(r.edges[e.edge as usize].vertices.map(|v| DVec3::from_array(r.vertices[v as usize].point).truncate()));
                    }
                }
            }
            let c = ps.iter().copied().sum::<glam::DVec2>() / ps.len().max(1) as f64;
            let j =
                (0..n).filter(|&j| sides[j].1.fillet > 0.0).min_by(|&a, &b| sides[a].1.p.distance(c).total_cmp(&sides[b].1.p.distance(c)));
            match j {
                Some(j) => format!("fillet:{}/{}", sides[(j + n - 1) % n].0, sides[j].0),
                None => format!("curved#{i}"),
            }
        }
    }
}

/// `solid` in the kernel, each face tagged `body:<face>`, then moved by `at`; `profile`, for a
/// prism, names its faces by its sides' ids (with its height).
fn leaf(k: &mut Kernel, body: &str, solid: &Solid, profile: Option<(&[(&str, Corner)], f64)>, at: DVec3) -> Brep {
    let r = solid.record();
    let (lo, hi) = solid.bounds().unwrap();
    let (mut brep, order) = solidcore_convert::brep::to_kernel(k, r, true).unwrap();
    for (i, &f) in order.faces.iter().enumerate() {
        let face = match profile {
            Some((sides, height)) => profile_face_name(r, i, sides, height),
            None => face_name(r, i, lo, hi),
        };
        let tag = Tag(BTreeSet::from([format!("{body}:{face}")]));
        brep.add_attribute(EntityRef::Face(f), Attribute::application(ID, Behavior::Copy, tag)).unwrap();
    }
    if at != DVec3::ZERO {
        let m = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [at.x, at.y, at.z, 1.0]];
        solidcore::topology::brep::mirror::transform_matrix(k, &mut brep, &m, false).unwrap();
    }
    brep
}

/// A step of a group: union or difference of a leaf.
enum Step<'a> {
    Add(&'a str, &'a Solid, DVec3),
    Cut(&'a str, &'a Solid, DVec3),
    /// A prism added, its faces named by its profile's side ids.
    AddProfiled(&'a str, &'a Solid, &'a [(&'a str, Corner)], f64),
}

/// What the trial reads back: each result face's tags, each result edge's name (the sorted
/// pair of its faces' tags), and how many edges carry each name.
struct Named {
    faces: Vec<String>,
    edges: BTreeMap<String, usize>,
    untagged: usize,
    /// Per name, its edges' end points: what a name points at, to tell a name kept from a name
    /// that now means another edge.
    ends: BTreeMap<String, Vec<[DVec3; 2]>>,
}

fn show(t: Option<&Tag>) -> String {
    match t {
        Some(t) => t.0.iter().cloned().collect::<Vec<_>>().join("|"),
        None => "∅".into(),
    }
}

/// The group's shape, in one kernel session.
fn build(k: &mut Kernel, steps: &[Step]) -> Brep {
    let mut shape: Option<Brep> = None;
    for s in steps {
        shape = Some(match s {
            Step::Add(n, solid, at) => {
                let l = leaf(k, n, solid, None, *at);
                match shape {
                    None => l,
                    Some(sh) => b::boolean_union(k, sh, l).unwrap(),
                }
            }
            Step::Cut(n, solid, at) => {
                let l = leaf(k, n, solid, None, *at);
                b::boolean_difference(k, shape.unwrap(), l).unwrap()
            }
            Step::AddProfiled(n, solid, sides, height) => {
                let l = leaf(k, n, solid, Some((sides, *height)), DVec3::ZERO);
                match shape {
                    None => l,
                    Some(sh) => b::boolean_union(k, sh, l).unwrap(),
                }
            }
        });
    }
    shape.unwrap()
}

/// The names read on `shape`, and each edge's name by its index in the native edge list.
fn names(k: &mut Kernel, shape: &mut Brep) -> (Named, Vec<String>) {
    let (rec, order) = solidcore_convert::brep::to_record(k, shape).unwrap();
    let tag = |i: usize| shape.find_attribute(EntityRef::Face(order.faces[i]), ID).and_then(|a| a.application_value::<Tag>().cloned());
    let face_tags: Vec<Option<Tag>> = (0..rec.faces.len()).map(tag).collect();
    let untagged = face_tags.iter().filter(|t| t.is_none()).count();
    // Each edge's faces, from the loops.
    let mut edge_faces: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); rec.edges.len()];
    for (fi, f) in rec.faces.iter().enumerate() {
        for l in &f.loops {
            if let Loop::Edges { edges } = l {
                for e in edges {
                    edge_faces[e.edge as usize].insert(fi);
                }
            }
        }
    }
    let native = shape.edge_list();
    let mut by_native = vec![String::new(); native.len()];
    let mut edges = BTreeMap::new();
    let mut ends: BTreeMap<String, Vec<[DVec3; 2]>> = BTreeMap::new();
    for (ei, fs) in edge_faces.iter().enumerate() {
        let mut names: Vec<String> = fs.iter().map(|&f| show(face_tags[f].as_ref())).collect();
        names.sort();
        let name = if names.len() == 1 { format!("{} (seam)", names[0]) } else { names.join(" × ") };
        if let Some(x) = native.iter().position(|&e| e == order.edges[ei]) {
            by_native[x] = name.clone();
        }
        let [a, b] = rec.edges[ei].vertices.map(|v| DVec3::from_array(rec.vertices[v as usize].point));
        ends.entry(name.clone()).or_default().push([a, b]);
        *edges.entry(name).or_insert(0) += 1;
    }
    let faces = face_tags.iter().map(|t| show(t.as_ref())).collect();
    (Named { faces, edges, untagged, ends }, by_native)
}

fn group(steps: &[Step]) -> Named {
    let mut k = kernel();
    let mut shape = build(&mut k, steps);
    names(&mut k, &mut shape).0
}

fn print(title: &str, n: &Named) {
    println!("\n== {title}: {} faces, {} untagged", n.faces.len(), n.untagged);
    let mut faces = n.faces.clone();
    faces.sort();
    println!("   faces: {}", faces.join(", "));
    for (name, count) in &n.edges {
        println!("   {count} × {name}");
    }
}

fn board() -> Solid {
    Solid::new_box(DVec3::ZERO, DVec3::new(100.0, 60.0, 20.0)).unwrap()
}

fn pocket() -> Solid {
    Solid::new_box(DVec3::ZERO, DVec3::new(30.0, 20.0, 15.0)).unwrap()
}

/// The board's top rim of the pocket: the four edges between the board's top and the
/// pocket's walls.
fn rim(n: &Named) -> usize {
    ["+X", "-X", "+Y", "-Y"].iter().map(|s| n.edges.get(&format!("board:+Z × pocket:{s}")).copied().unwrap_or(0)).sum()
}

#[test]
fn a_pocket_names_its_rim_by_the_board_top_and_its_walls_wherever_it_moves() {
    let (board, pocket) = (board(), pocket());
    let at = |x: f64| DVec3::new(x, 20.0, 10.0);
    let first = group(&[Step::Add("board", &board, DVec3::ZERO), Step::Cut("pocket", &pocket, at(20.0))]);
    print("pocket at x = 20", &first);
    let moved = group(&[Step::Add("board", &board, DVec3::ZERO), Step::Cut("pocket", &pocket, at(45.0))]);
    print("pocket moved to x = 45", &moved);
    assert_eq!(first.untagged, 0);
    assert_eq!(rim(&first), 4, "the rim's four edges named");
    assert_eq!(first.edges, moved.edges, "the same names after the move");
    // A face is one leaf's face: no result face carries two tags here.
    assert!(first.faces.iter().all(|f| !f.contains('|')), "{:?}", first.faces);
}

#[test]
fn a_pocket_breaking_out_of_the_side_loses_one_rim_edge() {
    let (board, pocket) = (board(), pocket());
    // Through the board's +X side: the rim edge on the pocket's +X wall is gone.
    let out = group(&[Step::Add("board", &board, DVec3::ZERO), Step::Cut("pocket", &pocket, DVec3::new(80.0, 20.0, 10.0))]);
    print("pocket out through +X", &out);
    assert_eq!(out.untagged, 0);
    assert_eq!(out.edges.get("board:+Z × pocket:+X"), None, "that name no longer resolves");
    assert_eq!(rim(&out), 3);
}

#[test]
fn a_hole_across_an_edge_splits_its_name_in_two() {
    let board = board();
    let hole = Solid::new_cylinder(DVec3::ZERO, 5.0, 30.0).unwrap();
    // A hole on the board's top/+X edge: that edge is cut in two pieces of one name.
    let cut = group(&[Step::Add("board", &board, DVec3::ZERO), Step::Cut("hole", &hole, DVec3::new(100.0, 30.0, -5.0))]);
    print("hole across the +Z/+X edge", &cut);
    assert_eq!(cut.untagged, 0);
    assert_eq!(cut.edges.get("board:+X × board:+Z"), Some(&2), "{:?}", cut.edges);
}

#[test]
fn a_union_names_its_inner_corner_and_merges_coplanar_tops() {
    let board = board();
    let upright = Solid::new_box(DVec3::ZERO, DVec3::new(20.0, 60.0, 80.0)).unwrap();
    // An upright on the board's end: the inner corner is board:+Z × upright:-X.
    let l = group(&[Step::Add("board", &board, DVec3::ZERO), Step::Add("upright", &upright, DVec3::new(80.0, 0.0, 20.0))]);
    print("L: upright on the board's end", &l);
    assert_eq!(l.untagged, 0);
    assert_eq!(l.edges.get("board:+Z × upright:-X"), Some(&1), "{:?}", l.edges);
    // Two boards side by side: their tops and fronts are coplanar.
    let two = group(&[Step::Add("a", &board, DVec3::ZERO), Step::Add("b", &board, DVec3::new(100.0, 0.0, 0.0))]);
    print("two boards side by side", &two);
    assert_eq!(two.untagged, 0);
}

/// The node over the group: a round of edges picked by name, the tags read again after it.
fn rounded(steps: &[Step], picked: &[&str], radius: f64) -> (Named, Named, std::time::Duration) {
    use solidcore_ops::blend::{Blend, Corners, blend};
    let mut k = kernel();
    let mut shape = build(&mut k, steps);
    let (before, by_native) = names(&mut k, &mut shape);
    let edges: Vec<usize> = (0..by_native.len()).filter(|&i| picked.contains(&by_native[i].as_str())).collect();
    assert!(!edges.is_empty(), "no edge named {picked:?}");
    let body = solidcore_ops::Body::from_kernel(shape);
    let t = std::time::Instant::now();
    let made = blend(&body, Blend::Round, &edges, &vec![radius; edges.len()], &[], Corners::Filled).unwrap();
    let took = t.elapsed();
    println!("   rounded {} edges r = {radius}: {} faces made, {took:?}", edges.len(), made.made.len());
    let mut guard = body.kernel();
    let (after, _) = names(&mut k, &mut guard);
    (before, after, took)
}

#[test]
fn a_round_over_the_group_keeps_the_tags_of_the_faces_it_trims() {
    let (board, pocket) = (board(), pocket());
    let steps = [Step::Add("board", &board, DVec3::ZERO), Step::Cut("pocket", &pocket, DVec3::new(35.0, 20.0, 10.0))];
    let top = ["board:+X × board:+Z", "board:+Y × board:+Z", "board:+Z × board:-X", "board:+Z × board:-Y"];
    let (_, after, _) = rounded(&steps, &top, 5.0);
    print("board − pocket, its top's outer edges rounded r = 5", &after);
    // The four round faces and their corners are new, untagged: the node names them itself.
    assert!(after.untagged >= 4, "{}", after.untagged);
    // The pocket's rim is still named as before.
    assert_eq!(rim(&after), 4);
    let rim_names = ["board:+Z × pocket:+X", "board:+Z × pocket:+Y", "board:+Z × pocket:-X", "board:+Z × pocket:-Y"];
    let (_, after, _) = rounded(&steps, &rim_names, 2.0);
    print("board − pocket, the pocket's rim rounded r = 2", &after);
}

// ---- A profiled leaf edited -----------------------------------------------------------------

/// Whether the edge `b` lies where the edge `a` did: the same ends, or both ends of `b` on
/// `a`'s line.
fn same_support(a: [DVec3; 2], b: [DVec3; 2]) -> bool {
    let near = |p: DVec3, q: DVec3| p.distance(q) < 1e-6;
    if (near(a[0], b[0]) && near(a[1], b[1])) || (near(a[0], b[1]) && near(a[1], b[0])) {
        return true;
    }
    let d = a[1] - a[0];
    if d.length() < 1e-9 {
        return false;
    }
    let off = |p: DVec3| (p - a[0]).cross(d).length() / d.length();
    off(b[0]) < 1e-6 && off(b[1]) < 1e-6
}

/// How the names of `before` fare in `after`: kept (same name, same place), gone, or moved
/// (same name, another edge: a node would round the wrong edge, silently); and the new names.
fn compare(before: &Named, after: &Named) -> (usize, Vec<String>, Vec<String>, Vec<String>) {
    let (mut kept, mut gone, mut moved) = (0, Vec::new(), Vec::new());
    for (name, was) in &before.ends {
        match after.ends.get(name) {
            None => gone.push(name.clone()),
            Some(now) if now.iter().all(|n| was.iter().any(|w| same_support(*w, *n))) => kept += 1,
            Some(_) => moved.push(name.clone()),
        }
    }
    let new = after.ends.keys().filter(|n| !before.ends.contains_key(*n)).cloned().collect();
    (kept, gone, moved, new)
}

/// An L outline (its inner corner d filleted 8 mm), each side with its id; `chamfered` cuts its
/// corner c by a chamfer, a side inserted between b→c and c→d. `positional` names the sides by
/// their index instead, as `profile::Corner` would today.
fn l_profile(chamfered: bool, positional: bool) -> Vec<(String, Corner)> {
    let c = |x: f64, y: f64| Corner::new(glam::DVec2::new(x, y));
    let mut d = c(40.0, 30.0);
    d.fillet = 8.0;
    let mut sides = vec![("s0", c(0.0, 0.0)), ("s1", c(100.0, 0.0))];
    if chamfered {
        // The side leaving c' is the chamfer (new id s6), the one leaving g is the old c→d (s2).
        sides.extend([("s6", c(100.0, 24.0)), ("s2", c(94.0, 30.0))]);
    } else {
        sides.push(("s2", c(100.0, 30.0)));
    }
    sides.extend([("s3", d), ("s4", c(40.0, 60.0)), ("s5", c(0.0, 60.0))]);
    sides.into_iter().enumerate().map(|(k, (id, corner))| (if positional { format!("p{k}") } else { id.to_string() }, corner)).collect()
}

fn profiled_group(chamfered: bool, positional: bool) -> Named {
    let sides = l_profile(chamfered, positional);
    let corners: Vec<Corner> = sides.iter().map(|(_, c)| *c).collect();
    let height = 20.0;
    let blank = Solid::new_prism(DVec3::ZERO, &corners, height).unwrap();
    let hole = Solid::new_cylinder(DVec3::ZERO, 5.0, 30.0).unwrap();
    let named: Vec<(&str, Corner)> = sides.iter().map(|(id, c)| (id.as_str(), *c)).collect();
    group(&[Step::AddProfiled("blank", &blank, &named, height), Step::Cut("hole", &hole, DVec3::new(20.0, 15.0, -5.0))])
}

fn report(title: &str, (kept, gone, moved, new): &(usize, Vec<String>, Vec<String>, Vec<String>)) {
    println!("\n== {title}: {kept} names kept");
    println!("   gone : {gone:?}");
    println!("   MOVED: {moved:?}");
    println!("   new  : {new:?}");
}

#[test]
fn a_profile_edit_keeps_its_names_by_side_ids_and_moves_them_by_positions() {
    let before = profiled_group(false, false);
    print("L profile (side ids) − hole", &before);
    assert_eq!(before.untagged, 0);
    assert!(before.faces.iter().all(|f| !f.contains('#')), "every face named by the profile: {:?}", before.faces);
    let after = profiled_group(true, false);
    print("L profile chamfered at c (side ids) − hole", &after);
    let ids = compare(&before, &after);
    report("side ids, corner c chamfered", &ids);
    assert!(ids.2.is_empty(), "no name moved: {:?}", ids.2);
    assert_eq!(ids.1, vec!["blank:side:s1 × blank:side:s2".to_string()], "only the vertical edge at c is gone");
    let pos = compare(&profiled_group(false, true), &profiled_group(true, true));
    report("positions, corner c chamfered", &pos);
    assert!(!pos.2.is_empty(), "positions rename silently");
}

// ---- A round over a round -------------------------------------------------------------------

/// Whether the edge named `edge` (its faces' tags, `a|b × c`) lies between faces tagged `p.0`
/// and `p.1`: by inclusion, a merged face carrying several tags.
fn matches(edge: &str, p: &(String, String)) -> bool {
    let sides: Vec<BTreeSet<&str>> = edge.split(" × ").map(|s| s.split('|').collect()).collect();
    let [x, y] = sides.as_slice() else { return false };
    (x.contains(p.0.as_str()) && y.contains(p.1.as_str())) || (x.contains(p.1.as_str()) && y.contains(p.0.as_str()))
}

/// A round node: the edges between the pairs of tags `picked`, at `radius`; the faces it makes
/// tagged `prefix(a/b)` after the pair of faces they lie between.
struct RoundNode {
    prefix: &'static str,
    picked: Vec<(String, String)>,
    radius: f64,
}

/// The faces `made` (native indices) tagged after the picked pair whose two faces they touch.
fn tag_made(k: &mut Kernel, shape: &mut Brep, made: &[usize], node: &RoundNode) -> Vec<String> {
    let (rec, order) = solidcore_convert::brep::to_record(k, shape).unwrap();
    let tags: Vec<BTreeSet<String>> = (0..rec.faces.len())
        .map(|i| {
            shape
                .find_attribute(EntityRef::Face(order.faces[i]), ID)
                .and_then(|a| a.application_value::<Tag>().map(|t| t.0.clone()))
                .unwrap_or_default()
        })
        .collect();
    let mut edge_faces: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); rec.edges.len()];
    for (fi, f) in rec.faces.iter().enumerate() {
        for l in &f.loops {
            if let Loop::Edges { edges } = l {
                for e in edges {
                    edge_faces[e.edge as usize].insert(fi);
                }
            }
        }
    }
    let mut out = Vec::new();
    let native: Vec<_> = shape.face_list().to_vec();
    for &m in made {
        let fi = order.faces.iter().position(|&f| f == native[m]).unwrap();
        let around: BTreeSet<&String> = edge_faces
            .iter()
            .filter(|fs| fs.contains(&fi))
            .flat_map(|fs| fs.iter().filter(|&&g| g != fi))
            .flat_map(|&g| tags[g].iter())
            .collect();
        let on: Vec<String> =
            node.picked.iter().filter(|(a, b)| around.contains(a) && around.contains(b)).map(|(a, b)| format!("{a}/{b}")).collect();
        let name = if on.is_empty() { format!("{}(corner)", node.prefix) } else { format!("{}({})", node.prefix, on.join(",")) };
        shape
            .add_attribute(EntityRef::Face(native[m]), Attribute::application(ID, Behavior::Copy, Tag(BTreeSet::from([name.clone()]))))
            .unwrap();
        out.push(name);
    }
    out
}

/// The group, then each node over the previous result; per node, how many edges its names
/// picked, the faces it made, its time; the final names.
fn nodes(steps: &[Step], nodes: &[RoundNode]) -> (Vec<(usize, Vec<String>, std::time::Duration)>, Named) {
    use solidcore_ops::blend::{Blend, Corners, blend};
    let mut k = kernel();
    let body = solidcore_ops::Body::from_kernel(build(&mut k, steps));
    let mut runs = Vec::new();
    for node in nodes {
        let by_native = names(&mut k, &mut body.kernel()).1;
        let edges: Vec<usize> = (0..by_native.len()).filter(|&i| node.picked.iter().any(|p| matches(&by_native[i], p))).collect();
        let t = std::time::Instant::now();
        let made = blend(&body, Blend::Round, &edges, &vec![node.radius; edges.len()], &[], Corners::Filled)
            .unwrap_or_else(|e| panic!("{} over {} edges: {e}", node.prefix, edges.len()));
        let took = t.elapsed();
        let made = tag_made(&mut k, &mut body.kernel(), &made.made, node);
        runs.push((edges.len(), made, took));
    }
    let named = names(&mut k, &mut body.kernel()).0;
    (runs, named)
}

fn pair(a: &str, b: &str) -> (String, String) {
    (a.to_string(), b.to_string())
}

/// r1 rounds the board's four upright edges; r2 its top's outer edges, which run over r1's
/// rounds: its names take r1's faces.
fn round_on_round(pocket_x: f64, r1: f64) -> (Vec<(usize, Vec<String>, std::time::Duration)>, Named) {
    let (board, pocket) = (board(), pocket());
    let steps = [Step::Add("board", &board, DVec3::ZERO), Step::Cut("pocket", &pocket, DVec3::new(pocket_x, 20.0, 10.0))];
    let corners = [("+X", "+Y"), ("+X", "-Y"), ("-X", "+Y"), ("-X", "-Y")];
    let first = RoundNode {
        prefix: "r1",
        picked: corners.iter().map(|(a, b)| pair(&format!("board:{a}"), &format!("board:{b}"))).collect(),
        radius: r1,
    };
    let mut top: Vec<(String, String)> = ["+X", "-X", "+Y", "-Y"].iter().map(|s| pair("board:+Z", &format!("board:{s}"))).collect();
    top.extend(corners.iter().map(|(a, b)| pair("board:+Z", &format!("r1(board:{a}/board:{b})"))));
    let second = RoundNode { prefix: "r2", picked: top, radius: 3.0 };
    nodes(&steps, &[first, second])
}

#[test]
fn a_round_over_a_round_picks_its_edges_by_the_first_rounds_faces() {
    let (runs, named) = round_on_round(35.0, 10.0);
    for (i, (picked, made, took)) in runs.iter().enumerate() {
        println!("\n   node r{}: {picked} edges picked, {} faces made in {took:?}: {made:?}", i + 1, made.len());
    }
    print("board − pocket, r1 = 10 on the uprights, r2 = 3 on the top over them", &named);
    assert_eq!(runs[0].0, 4);
    assert_eq!(runs[1].0, 8, "the top's 4 straight edges and the 4 arcs over r1");
    assert_eq!(named.untagged, 0, "every face named, the rounds' by their nodes");
    // The pocket moved and r1 changed: every node finds its edges again by the same names.
    let (again, renamed) = round_on_round(50.0, 12.0);
    print("pocket moved to x = 50, r1 = 12", &renamed);
    assert_eq!(again.iter().map(|r| r.0).collect::<Vec<_>>(), vec![4, 8]);
    assert_eq!(named.edges, renamed.edges, "the same names");
}

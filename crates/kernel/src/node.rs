//! The node task (docs/decisions/0042-body-nodes-operators.md): a node's children computed from
//! their leaves in one kernel session, the leaves' faces tagged by their recipe's names and the
//! tags carried through the booleans by an application attribute of SolidCore's; then the
//! node's operation applied to the edges its names match, and the faces it made tagged in turn.

use std::collections::BTreeSet;

use glam::{DAffine3, DVec3};
use solidcore::boolean::api_brep as b;
use solidcore::geom::attribute::{
    ApplicationValue, Attribute, AttributeHandle, AttributeHandlers, AttributeValue, AttributeView, Behavior, ObjectKey, Placement, ids,
};
use solidcore::topology::brep::p1::num_faces;
use solidcore::{Brep, EntityRef, Kernel};
use solidcore_record::brep::{Brep as Record, Loop};

use crate::engine::{guarded, kernel, kernel_err, row_major, to_record_ordered};
use crate::names::{
    Blend, BlendOp, EdgeName, FaceMap, FaceName, FaceTag, InputNames, NodeEdges, NodeOp, NodeRefusal, TreeBody, TreeNode, leaf_faces, under,
};
use crate::{Error, Result, Solid};

/// The attribute the tags are stored under, among the application's ids.
const TAG: u64 = ids::USER_1 + 42;

/// The leaf faces a face comes from: one, or several where faces merged or one lay on another.
#[derive(Clone, Debug, PartialEq)]
struct Tag(BTreeSet<FaceTag>);

fn tag_on(view: &AttributeView<'_>, x: ObjectKey) -> Option<(AttributeHandle, Tag)> {
    let (h, a) = view.find_attribute(x, TAG)?;
    a.application_value::<Tag>().map(|t| (h, t.clone()))
}

/// `x` given `tag` joined to what it carries.
fn join_onto(view: &AttributeView<'_>, x: ObjectKey, tag: &Tag) -> Vec<Placement> {
    match tag_on(view, x) {
        Some((handle, now)) => {
            let joined = Tag(now.0.union(&tag.0).cloned().collect());
            if joined == now {
                return Vec::new();
            }
            vec![Placement::Edit { handle, value: AttributeValue::Application(ApplicationValue::new(joined)) }]
        }
        None => vec![Placement::Add { on: x, attribute: Attribute::application(TAG, Behavior::Copy, tag.clone()) }],
    }
}

impl AttributeHandlers for Tag {
    // A split: the default, each piece keeps the tag.

    /// The store calls one merge for both sources: each is read from the view, and joined.
    fn merge(&self, view: &AttributeView<'_>, a: ObjectKey, b: ObjectKey, merged: ObjectKey) -> solidcore::Result<Vec<Placement>> {
        let mut all = self.0.clone();
        for x in [a, b, merged] {
            if let Some((_, t)) = tag_on(view, x) {
                all.extend(t.0);
            }
        }
        Ok(join_onto(view, merged, &Tag(all)))
    }

    fn coincident(&self, view: &AttributeView<'_>, _from: ObjectKey, to: ObjectKey) -> solidcore::Result<Vec<Placement>> {
        Ok(join_onto(view, to, self))
    }
}

fn tag(b: &mut Brep, f: solidcore::topology::FaceId, tags: BTreeSet<FaceTag>, call: &str) -> Result<()> {
    if let Some(h) = b.entity_attributes(EntityRef::Face(f)).into_iter().find(|(_, a)| a.attribute_id() == TAG).map(|(h, _)| h) {
        b.remove_attribute(EntityRef::Face(f), h).map_err(|e| kernel_err(call, e))?;
    }
    b.add_attribute(EntityRef::Face(f), Attribute::application(TAG, Behavior::Copy, Tag(tags))).map_err(|e| kernel_err(call, e))?;
    Ok(())
}

// ---- the session ------------------------------------------------------------------------------

/// Each face of `b` (by record index) with its tags, each edge with its faces, and the record's
/// order: what names are read on.
struct Read {
    record: Record,
    order: solidcore_convert::brep::Order,
    tags: Vec<Vec<FaceTag>>,
    edge_faces: Vec<Vec<usize>>,
}

fn read(k: &mut Kernel, b: &mut Brep, call: &str) -> Result<Read> {
    let (record, order) =
        solidcore_convert::brep::to_record(k, b).map_err(|e| Error::Convert { call: call.into(), message: e.to_string() })?;
    let tags = order
        .faces
        .iter()
        .map(|&f| {
            b.find_attribute(EntityRef::Face(f), TAG)
                .and_then(|a| a.application_value::<Tag>())
                .map(|t| t.0.iter().cloned().collect())
                .unwrap_or_default()
        })
        .collect();
    let mut edge_faces = vec![Vec::new(); record.edges.len()];
    for (fi, f) in record.faces.iter().enumerate() {
        for l in &f.loops {
            if let Loop::Edges { edges } = l {
                for e in edges {
                    let list: &mut Vec<usize> = &mut edge_faces[e.edge as usize];
                    if !list.contains(&fi) {
                        list.push(fi);
                    }
                }
            }
        }
    }
    Ok(Read { record, order, tags, edge_faces })
}

impl Read {
    /// The record edge of the kernel edge `e`.
    fn edge_of(&self, e: solidcore::topology::EdgeId) -> Option<usize> {
        self.order.edges.iter().position(|&x| x == e)
    }

    /// The tags of the two faces of record edge `e`, none for a seam or a wire.
    fn faces_of(&self, e: usize) -> Option<(&[FaceTag], &[FaceTag])> {
        match self.edge_faces[e].as_slice() {
            [a, b] => Some((&self.tags[*a], &self.tags[*b])),
            _ => None,
        }
    }
}

/// The group `children` computed in the session `k`, their leaves' faces tagged from `path`
/// down; `skip` the bodies (paths) left out, `at` the body being added (for a failure).
fn group(k: &mut Kernel, children: &[TreeBody], path: &[u64], cx: &mut Session) -> Result<Option<Brep>> {
    let call = "node";
    let mut shape: Option<Brep> = None;
    // The positives, then the negatives, then the intersections (docs/decisions/0045-intersection-a-third-sign.md).
    let order = children
        .iter()
        .filter(|c| !c.negative && !c.intersect)
        .chain(children.iter().filter(|c| c.negative))
        .chain(children.iter().filter(|c| c.intersect));
    for c in order {
        let here: Vec<u64> = path.iter().copied().chain([c.id]).collect();
        if cx.skip.contains(&here) {
            continue;
        }
        cx.at = Some(here.clone());
        let Some(mut body) = (match &c.node {
            TreeNode::Leaf { solid, recipe } => tagged(k, cx.inputs[*solid as usize], &here, &InputNames::Recipe(recipe.clone()), call)?,
            // A group or a node computed already: its cache, named by its face map.
            TreeNode::Mapped { solid, faces } => tagged(k, cx.inputs[*solid as usize], &here, &InputNames::Map(faces.clone()), call)?,
            // The node's children's result already: named from the node, not from a child.
            TreeNode::Base { solid, faces } => tagged(k, cx.inputs[*solid as usize], path, &InputNames::Map(faces.clone()), call)?,
            TreeNode::Group { children, op } => {
                let inner = group(k, children, &here, cx)?;
                let inner = match (inner, op) {
                    // Refused, its children's result goes on, and the node is told why.
                    (Some(b), Some(op)) => {
                        let (b, _, refusal) = apply(k, b, op, &[], &here)?;
                        if let Some(why) = refusal {
                            cx.inner.push((here.clone(), why));
                        }
                        Some(b)
                    }
                    (inner, _) => inner,
                };
                // Read back and tagged afresh, as a group computed already is: taken as it
                // came out of its booleans, lumps of it flush with the shape it then cuts had
                // their walls tagged with that shape's faces.
                match inner {
                    Some(mut b) => {
                        let (solid, map) = read_map(k, &mut b, call)?;
                        tagged(k, &solid, &[], &InputNames::Map(map), call)?
                    }
                    None => None,
                }
            }
        }) else {
            continue;
        };
        if !c.placement.abs_diff_eq(DAffine3::IDENTITY, 1e-12) && !matches!(c.node, TreeNode::Base { .. }) {
            solidcore::topology::brep::mirror::transform_matrix(k, &mut body, &row_major(&c.placement), false)
                .map_err(|e| kernel_err(call, e))?;
        }
        let adds = !c.negative && !c.intersect;
        shape = Some(match (shape.take(), adds) {
            (None, true) => body,
            (Some(s), true) => b::boolean_union(k, s, body).map_err(|e| kernel_err(call, e))?,
            (Some(s), false) => {
                let cut = match c.intersect {
                    true => b::boolean_intersection(k, s, body),
                    false => b::boolean_difference(k, s, body),
                }
                .map_err(|e| kernel_err(call, e))?;
                if num_faces(&cut) == 0 {
                    cx.at = None;
                    return Ok(None);
                }
                cut
            }
            // A machining or an intersection with nothing to act on yet: its positives all failed or
            // are empty.
            (None, false) => continue,
        });
    }
    cx.at = None;
    Ok(shape)
}

/// `s` in the session `k`, its faces tagged from `path` down by `names` (a leaf's recipe, or a
/// computed group's face map); none when it is empty.
fn tagged(k: &mut Kernel, s: &Solid, path: &[u64], names: &InputNames, call: &str) -> Result<Option<Brep>> {
    if s.is_empty() {
        return Ok(None);
    }
    let (mut b, order) = solidcore_convert::brep::to_kernel(k, s.record(), true)
        .map_err(|e| Error::Convert { call: call.into(), message: e.to_string() })?;
    let leaf = match names {
        InputNames::Recipe(recipe) => leaf_faces(s.record(), recipe.as_ref()),
        InputNames::Map(_) => Vec::new(),
    };
    for (i, &f) in order.faces.iter().enumerate() {
        let tags: BTreeSet<FaceTag> = match names {
            InputNames::Recipe(_) => {
                BTreeSet::from([FaceTag { path: path.to_vec(), face: leaf.get(i).cloned().unwrap_or(FaceName::Face(i as u32)) }])
            }
            InputNames::Map(map) => map.get(i).into_iter().flatten().map(|t| under(path, t)).collect(),
        };
        if !tags.is_empty() {
            tag(&mut b, f, tags, call)?;
        }
    }
    Ok(Some(b))
}

/// [`tagged`], then placed by `t`: a recipe's input (`engine::recipe_session`).
pub(crate) fn tagged_placed(k: &mut Kernel, s: &Solid, t: &DAffine3, path: &[u64], names: &InputNames, call: &str) -> Result<Brep> {
    let mut b = tagged(k, s, path, names, call)?.ok_or_else(|| Error::Convert { call: call.into(), message: "an empty input".into() })?;
    if !t.abs_diff_eq(DAffine3::IDENTITY, 1e-12) {
        solidcore::topology::brep::mirror::transform_matrix(k, &mut b, &row_major(t), false).map_err(|e| kernel_err(call, e))?;
    }
    Ok(b)
}

/// `b` as a record, with its face map: each record face's tags.
pub(crate) fn read_map(k: &mut Kernel, b: &mut Brep, call: &str) -> Result<(Solid, FaceMap)> {
    let (solid, order) = to_record_ordered(k, b, call)?;
    let map = order
        .faces
        .iter()
        .map(|&f| {
            b.find_attribute(EntityRef::Face(f), TAG)
                .and_then(|a| a.application_value::<Tag>())
                .map(|t| t.0.iter().cloned().collect())
                .unwrap_or_default()
        })
        .collect();
    Ok((solid, map))
}

/// What a session reads and records.
struct Session<'a> {
    inputs: &'a [&'a Solid],
    skip: Vec<Vec<u64>>,
    at: Option<Vec<u64>>,
    /// The nodes inside, computed from their leaves, that were refused: by path, and why.
    inner: Vec<(Vec<u64>, NodeRefusal)>,
}

/// `op` applied to `shape`, the result of the node at `path` (its names relative to it); with
/// `picks`, its edges taken nearest those points instead (the tool's), and named. The shape as
/// it came when the node is refused, and why; the edges it took, by name.
fn apply(
    k: &mut Kernel,
    shape: Brep,
    op: &NodeOp,
    picks: &[[f64; 3]],
    path: &[u64],
) -> Result<(Brep, Vec<NodeEdges>, Option<NodeRefusal>)> {
    match op {
        NodeOp::Blend(b) => apply_blend(k, shape, b, picks, path),
    }
}

/// [`apply`] for a round or a chamfer.
fn apply_blend(
    k: &mut Kernel,
    mut shape: Brep,
    op: &BlendOp,
    picks: &[[f64; 3]],
    path: &[u64],
) -> Result<(Brep, Vec<NodeEdges>, Option<NodeRefusal>)> {
    use solidcore_ops::blend::{Corners, blend};
    let call = "node";
    let before = read(k, &mut shape, call)?;
    let native: Vec<solidcore::topology::EdgeId> = shape.edge_list().to_vec();
    let record_of: Vec<Option<usize>> = native.iter().map(|&e| before.edge_of(e)).collect();
    // The name of native edge `x`, relative to the node: a tag of each face — of a merged face,
    // the one nearest the other face's in the tree (most of their paths in common: of one body
    // first), not the first by id. An edge of board a along a top merged with board b's would
    // else be named by b's top, and lost once b moved away.
    let strip = |t: &FaceTag| FaceTag { path: t.path[path.len().min(t.path.len())..].to_vec(), face: t.face.clone() };
    let name_of = |x: usize| -> Option<EdgeName> {
        let (a, b) = before.faces_of(record_of[x]?)?;
        let common = |s: &FaceTag, t: &FaceTag| s.path.iter().zip(&t.path).take_while(|(p, q)| p == q).count();
        // The first of the best pairs (`max_by_key` keeps the last of equals: reversed).
        let pairs = a.iter().flat_map(|s| b.iter().map(move |t| (s, t))).collect::<Vec<_>>();
        let (s, t) = pairs.into_iter().rev().max_by_key(|(s, t)| common(s, t))?;
        Some(EdgeName::new(strip(s), strip(t)))
    };
    let matching = |name: &EdgeName| -> Vec<usize> {
        let placed = EdgeName { faces: [under(path, &name.faces[0]), under(path, &name.faces[1])] };
        (0..native.len()).filter(|&x| record_of[x].and_then(|e| before.faces_of(e)).is_some_and(|(a, b)| placed.matches(a, b))).collect()
    };
    let body = solidcore_ops::Body::from_kernel(shape);
    let mut edges: Vec<usize> = Vec::new();
    let mut taken: Vec<NodeEdges> = Vec::new();
    if !picks.is_empty() {
        let picked = match solidcore_ops::pick::edges(&body, picks, 0.5) {
            Ok(Ok(p)) => p,
            _ => return Ok((std::mem::take(&mut *body.kernel()), op.edges.clone(), Some(NodeRefusal::NoEdge))),
        };
        // By name: all its edges, or only those picked (their points kept).
        let mut by_name: Vec<(EdgeName, Vec<usize>, Vec<[f64; 3]>)> = Vec::new();
        for (x, p) in picked.iter().zip(picks) {
            let Some(name) = name_of(*x) else { continue };
            match by_name.iter_mut().find(|(n, ..)| *n == name) {
                Some((_, xs, ps)) => {
                    if !xs.contains(x) {
                        xs.push(*x);
                        ps.push(*p);
                    }
                }
                None => by_name.push((name, vec![*x], vec![*p])),
            }
        }
        for (name, xs, ps) in by_name {
            let all = matching(&name);
            let whole = all.iter().all(|x| xs.contains(x));
            edges.extend(if whole { all } else { xs });
            taken.push(NodeEdges { name, near: if whole { Vec::new() } else { ps } });
        }
    } else {
        let mut lost = Vec::new();
        for e in &op.edges {
            let all = matching(&e.name);
            if all.is_empty() {
                lost.push(e.name.clone());
                continue;
            }
            if e.near.is_empty() {
                edges.extend(all);
            } else {
                // The kernel's distance to the edge itself: a chord between its ends says
                // nothing of a whole circle (its two ends one vertex).
                let shape = body.kernel();
                for p in &e.near {
                    let p = solidcore::Point3::new(p[0], p[1], p[2]);
                    let d = |x: usize| {
                        solidcore::query::api_queries::edge_closest_point(k, &shape, native[x], p)
                            .map_or(f64::INFINITY, |c| c.closest.distance)
                    };
                    if let Some(x) = all.iter().copied().min_by(|&a, &b| d(a).total_cmp(&d(b))) {
                        edges.push(x);
                    }
                }
            }
        }
        taken = op.edges.clone();
        if !lost.is_empty() {
            return Ok((std::mem::take(&mut *body.kernel()), taken, Some(NodeRefusal::Lost(lost))));
        }
    }
    edges.sort_unstable();
    edges.dedup();
    if edges.is_empty() {
        return Ok((std::mem::take(&mut *body.kernel()), taken, Some(NodeRefusal::NoEdge)));
    }
    let kind = match op.kind {
        Blend::Round => solidcore_ops::blend::Blend::Round,
        Blend::Chamfer => solidcore_ops::blend::Blend::Chamfer,
    };
    let made = match blend(&body, kind, &edges, &vec![op.size; edges.len()], &[], Corners::Filled) {
        Ok(m) => m.made,
        Err(e) => {
            let why = match e {
                solidcore_ops::Error::TooLarge { size, .. } => NodeRefusal::TooLarge { size: Some(size) },
                solidcore_ops::Error::NoRoom => NodeRefusal::TooLarge { size: None },
                e => NodeRefusal::Kernel(e.to_string()),
            };
            return Ok((std::mem::take(&mut *body.kernel()), taken, Some(why)));
        }
    };
    let mut shape = std::mem::take(&mut *body.kernel());
    // The faces made, named in two passes. A round's (a chamfer's) by the edge it replaces: the
    // taken name whose two faces run longest along it — its rails, not the faces its ends meet.
    // Then a corner's patch, where rounds meet, by the faces around it, the rounds named first
    // (they are all it touches).
    let after = read(k, &mut shape, call)?;
    let faces: Vec<solidcore::topology::FaceId> = shape.face_list().to_vec();
    let made: Vec<(solidcore::topology::FaceId, usize)> = made
        .iter()
        .filter_map(|&m| {
            let f = *faces.get(m)?;
            Some((f, after.order.faces.iter().position(|&x| x == f)?))
        })
        .collect();
    // The faces next to record face `fi`, each with the length (chords) of the edges they share.
    let next_to = |fi: usize| -> Vec<(usize, f64)> {
        let mut out: Vec<(usize, f64)> = Vec::new();
        for (e, fs) in after.edge_faces.iter().enumerate().filter(|(_, fs)| fs.contains(&fi)) {
            let [a, b] = after.record.edges[e].vertices.map(|v| DVec3::from_array(after.record.vertices[v as usize].point));
            for &g in fs.iter().filter(|&&g| g != fi) {
                match out.iter_mut().find(|(h, _)| *h == g) {
                    Some((_, l)) => *l += a.distance(b),
                    None => out.push((g, a.distance(b))),
                }
            }
        }
        out
    };
    let mut named: Vec<Option<FaceTag>> = vec![None; made.len()];
    for (k, &(_, fi)) in made.iter().enumerate() {
        let next = next_to(fi);
        // How long the faces carrying `t` run along it; none when none touches it.
        let along = |t: &FaceTag| {
            let touching: Vec<f64> = next.iter().filter(|(g, _)| after.tags[*g].contains(t)).map(|(_, l)| *l).collect();
            (!touching.is_empty()).then(|| touching.iter().sum::<f64>())
        };
        let best = taken
            .iter()
            .filter_map(|t| {
                let [a, b] = [under(path, &t.name.faces[0]), under(path, &t.name.faces[1])];
                Some((along(&a)?.min(along(&b)?), [strip(&a), strip(&b)]))
            })
            // The first of the longest (`max_by` keeps the last of equals: reversed).
            .rev()
            .max_by(|x, y| x.0.total_cmp(&y.0));
        named[k] = best.map(|(_, between)| FaceTag { path: path.to_vec(), face: FaceName::Made(Box::new(between)) });
    }
    for k in 0..made.len() {
        if named[k].is_some() {
            continue;
        }
        let around: BTreeSet<FaceTag> = next_to(made[k].1)
            .into_iter()
            .flat_map(|(g, _)| match made.iter().position(|&(_, x)| x == g) {
                Some(j) => named[j].iter().cloned().collect::<Vec<_>>(),
                None => after.tags[g].clone(),
            })
            .map(|t| strip(&t))
            .collect();
        if !around.is_empty() {
            named[k] = Some(FaceTag { path: path.to_vec(), face: FaceName::Corner(around.into_iter().collect()) });
        }
    }
    for (&(f, _), name) in made.iter().zip(named) {
        if let Some(name) = name {
            tag(&mut shape, f, BTreeSet::from([name]), call)?;
        }
    }
    Ok((shape, taken, None))
}

/// What [`crate::Task::Node`] gives: the node's solid, the edges it took by name, the children
/// whose boolean failed (by path), and why it left its children's result as it was.
pub(crate) struct NodeResult {
    pub solid: Solid,
    pub faces: FaceMap,
    pub edges: Vec<NodeEdges>,
    pub failed: Vec<Vec<u64>>,
    pub refusal: Option<NodeRefusal>,
    /// The nodes inside it computed with it that were refused: by path, and why.
    pub inner: Vec<(Vec<u64>, NodeRefusal)>,
    /// Its children's result, before its operation, and its face map: for the node to start
    /// from next time its children are the same, or only cutters were added
    /// ([`TreeNode::Base`]).
    pub raw: Option<(Solid, FaceMap)>,
}

/// [`crate::Task::Node`]: `children` computed in one session, then `op` (with `picks`, its
/// edges taken there); a child whose boolean fails is skipped and the session started again.
pub(crate) fn node(inputs: &[&Solid], children: &[TreeBody], op: &NodeOp, picks: &[[f64; 3]]) -> Result<NodeResult> {
    let mut skip: Vec<Vec<u64>> = Vec::new();
    loop {
        let mut cx = Session { inputs, skip: skip.clone(), at: None, inner: Vec::new() };
        let r = guarded("node", || {
            let mut k = kernel();
            let Some(shape) = group(&mut k, children, &[], &mut cx)? else {
                return Ok(NodeResult {
                    solid: Solid::empty(),
                    faces: Vec::new(),
                    edges: op.edges().to_vec(),
                    failed: Vec::new(),
                    refusal: None,
                    inner: std::mem::take(&mut cx.inner),
                    raw: None,
                });
            };
            cx.at = None;
            let mut shape = shape;
            // Its children's result, unless it is the one it was given as it is.
            let given = matches!(children, [TreeBody { node: TreeNode::Base { .. }, .. }]);
            let raw = if given { None } else { Some(read_map(&mut k, &mut shape, "node")?) };
            let (mut shape, edges, refusal) = apply(&mut k, shape, op, picks, &[])?;
            let (solid, faces) = read_map(&mut k, &mut shape, "node")?;
            Ok(NodeResult { solid, faces, edges, failed: Vec::new(), refusal, inner: std::mem::take(&mut cx.inner), raw })
        });
        match r {
            Ok(mut r) => {
                r.failed = skip;
                return Ok(r);
            }
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => match cx.at.take() {
                Some(at) if !skip.contains(&at) => skip.push(at),
                _ => return Err(e),
            },
        }
    }
}

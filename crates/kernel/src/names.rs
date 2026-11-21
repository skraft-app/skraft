//! Names of faces and edges in a body node's result, and the node task's input
//! (docs/decisions/0042-body-nodes-operators.md): a node takes pieces of its children's result
//! by their names, which hold while the children are edited.
//!
//! A leaf's faces are named by its recipe (a profile's caps and sides by their ids, a free
//! shape's by their record index), tagged with the path of bodies from the node down to the
//! leaf; the tags go through the booleans of the session computing the node (a face cut in two
//! keeps its tag, two merged carry both). An edge is named by the tags of its two faces, and
//! matches by inclusion: an edge whose faces carry those tags, a merged face carrying several.

use glam::{DAffine3, DVec2, DVec3};
use serde::{Deserialize, Serialize};
use solidcore_record::brep::{Brep as Record, Loop, Surface};

use crate::Primitive;
use crate::profile::{self, SideName};

/// A face of a leaf as its recipe names it, or a face a node made.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum FaceName {
    /// A profile's extrusion caps.
    Bottom,
    Top,
    /// The face a profile's side makes, by the side's id ([`crate::profile::Corner::id`]).
    Side(u32),
    /// The face a profile corner's fillet makes, by that corner's id.
    Fillet(u32),
    /// A face of a free shape, by its index in the record: it holds as long as the shape does.
    Face(u32),
    /// A face a node made (a round's, a chamfer's), by the two faces it lies between, named in
    /// that node's children.
    Made(Box<[FaceTag; 2]>),
    /// A corner's patch a node made where its rounds meet, by the faces around it (its rounds
    /// among them), named in that node's children.
    Corner(Vec<FaceTag>),
}

/// A face named in a node's children: the bodies from the node down (ids), and the face.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FaceTag {
    pub path: Vec<u64>,
    pub face: FaceName,
}

/// An edge of a node's children's result, by the faces it lies between (sorted).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EdgeName {
    pub faces: [FaceTag; 2],
}

impl EdgeName {
    pub fn new(a: FaceTag, b: FaceTag) -> EdgeName {
        if a <= b { EdgeName { faces: [a, b] } } else { EdgeName { faces: [b, a] } }
    }

    /// Is an edge whose faces carry the tags `x` and `y` (each face one or several, merged) of
    /// this name?
    pub fn matches(&self, x: &[FaceTag], y: &[FaceTag]) -> bool {
        let [a, b] = &self.faces;
        (x.contains(a) && y.contains(b)) || (x.contains(b) && y.contains(a))
    }
}

/// The edges of one name a node takes: all of them, or (`near` given: only some were picked)
/// the one nearest each point, in the node's axes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeEdges {
    pub name: EdgeName,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub near: Vec<[f64; 3]>,
}

/// A blend's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Blend {
    /// A round: the size its radius.
    Round,
    /// A chamfer at equal distances along both faces: the size the distance.
    Chamfer,
}

/// What a node does to its children's result (docs/decisions/0042-body-nodes-operators.md).
/// One kind yet, others to come (a shell); not the intersection, a sign of bodies, nor the
/// mirror, a placement (docs/REVIEW.md, 2026-11-20).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeOp {
    /// Edges rounded or chamfered.
    Blend(BlendOp),
}

/// A round or a chamfer: its kind, its size, the edges it takes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlendOp {
    pub kind: Blend,
    pub size: f64,
    pub edges: Vec<NodeEdges>,
}

impl NodeOp {
    /// A round or a chamfer of `size` taking `edges`.
    pub fn blend_of(kind: Blend, size: f64, edges: Vec<NodeEdges>) -> NodeOp {
        NodeOp::Blend(BlendOp { kind, size, edges })
    }

    /// Its blend, when it is one.
    pub fn blend(&self) -> Option<&BlendOp> {
        match self {
            NodeOp::Blend(b) => Some(b),
        }
    }

    pub fn blend_mut(&mut self) -> Option<&mut BlendOp> {
        match self {
            NodeOp::Blend(b) => Some(b),
        }
    }

    /// The edges it takes by name: those of its children's result it names.
    pub fn edges(&self) -> &[NodeEdges] {
        match self {
            NodeOp::Blend(b) => &b.edges,
        }
    }

    /// The same, taking `edges` instead (as the kernel named them).
    pub fn with_edges(self, edges: Vec<NodeEdges>) -> NodeOp {
        match self {
            NodeOp::Blend(b) => NodeOp::Blend(BlendOp { edges, ..b }),
        }
    }

    /// Every face name it holds, to rewrite (bodies moved under it).
    pub fn tags_mut(&mut self) -> impl Iterator<Item = &mut FaceTag> {
        match self {
            NodeOp::Blend(b) => b.edges.iter_mut().flat_map(|e| e.name.faces.iter_mut()),
        }
    }
}

/// A body of a node's children, as the node task computes it: its id (for the names' paths),
/// sign and placement in its parent's axes, and what it is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TreeBody {
    pub id: u64,
    pub negative: bool,
    /// An intersection (a ∩ body): it limits its level, intersected after the positives and negatives
    /// (docs/decisions/0045-intersection-a-third-sign.md); never `negative` too.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub intersect: bool,
    pub placement: DAffine3,
    pub node: TreeNode,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TreeNode {
    /// `inputs[solid]`, its faces named by `recipe` (a profile's primitive, in the solid's axes)
    /// or, without one, by their record index.
    Leaf { solid: u32, recipe: Option<Primitive> },
    /// A group: `(∪ positive children) − (∪ negative children)`, then `op` when it is a node.
    Group { children: Vec<TreeBody>, op: Option<NodeOp> },
    /// A group or a node computed already: `inputs[solid]`, its cached result, its faces named
    /// by its face map (relative to it), not computed again from its leaves.
    Mapped { solid: u32, faces: FaceMap },
    /// The node's own children's result computed already (`inputs[solid]`, its faces named by
    /// `faces`, relative to the node): what the bodies after it join or cut, as the first
    /// positive. Its body's id and placement are not used: it is the node's, not a child's.
    Base { solid: u32, faces: FaceMap },
}

/// The names of a solid's faces, by record face index: each face's tags (several where faces
/// merged), relative to the group or node whose result it is
/// (docs/decisions/0042-body-nodes-operators.md: names live beside the caches).
pub type FaceMap = Vec<Vec<FaceTag>>;

/// How a recipe's input names its faces: by its recipe (a leaf, [`leaf_faces`]), or by the
/// face map of the group it is the result of.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum InputNames {
    Recipe(Option<Primitive>),
    Map(FaceMap),
}

/// A group's recipe named: each input's body id and names, positives, negatives then intersections,
/// as the recipe lists them. The recipe then gives its result's face map.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecipeNames {
    pub positives: Vec<(u64, InputNames)>,
    pub negatives: Vec<(u64, InputNames)>,
    /// The ∩ bodies' (docs/decisions/0045-intersection-a-third-sign.md).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub intersects: Vec<(u64, InputNames)>,
}

/// A cheap fingerprint of a record: its counts and its vertices. A face map is used only on
/// the solid it was made for (one with this fingerprint). Written in the file: the same on
/// every platform and Rust release (FNV-1a over `u64`s, not `DefaultHasher` over `usize`s,
/// whose hashes differ between wasm32 and 64 bits and are not promised to last).
pub fn fingerprint(r: &Record) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |x: u64| {
        for b in x.to_le_bytes() {
            h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for n in [r.faces.len(), r.edges.len(), r.vertices.len()] {
        eat(n as u64);
    }
    for v in &r.vertices {
        v.point.iter().for_each(|c| eat(c.to_bits()));
    }
    h
}

/// `tag` as seen from `path` up: `path` before its own.
pub fn under(path: &[u64], tag: &FaceTag) -> FaceTag {
    FaceTag { path: path.iter().copied().chain(tag.path.iter().copied()).collect(), face: tag.face.clone() }
}

/// The face map of a leaf's solid `r` (built from `recipe`), seen from its parent: its body
/// `id` before each name.
pub fn leaf_map(r: &Record, recipe: Option<&Primitive>, id: u64) -> FaceMap {
    leaf_faces(r, recipe).into_iter().map(|face| vec![FaceTag { path: vec![id], face }]).collect()
}

/// Why a node left its children's result as it was.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NodeRefusal {
    /// Names that no longer match an edge.
    Lost(Vec<EdgeName>),
    /// No edge to take (none picked, or a pick near no edge).
    NoEdge,
    /// The size is too large: `size` is the largest that builds, when the kernel says.
    TooLarge { size: Option<f64> },
    /// The kernel refused it otherwise.
    Kernel(String),
}

/// Each leaf's `solid` index of `bodies`, at any depth, replaced by `f` of it.
pub fn map_leaves(bodies: &mut [TreeBody], f: &mut dyn FnMut(u32) -> u32) {
    for b in bodies {
        match &mut b.node {
            TreeNode::Leaf { solid, .. } | TreeNode::Mapped { solid, .. } | TreeNode::Base { solid, .. } => *solid = f(*solid),
            TreeNode::Group { children, .. } => map_leaves(children, f),
        }
    }
}

// ---- a leaf's names --------------------------------------------------------------------------

/// A point standing for face `i` of `r`: its vertices' centroid, or (`inside`) a point inside
/// it where its surface tells — a plane's middle (its domain's), the mean of a curved face's
/// control points (on the side it bulges to, where the vertices are those of a chord).
fn sample(r: &Record, i: usize, inside: bool) -> DVec3 {
    let f = &r.faces[i];
    let middle = |k: usize| (f.domain[k][0] + f.domain[k][1]) / 2.0;
    match f.surface {
        Surface::Plane { origin, u_axis, v_axis } if inside => {
            return DVec3::from_array(origin) + DVec3::from_array(u_axis) * middle(0) + DVec3::from_array(v_axis) * middle(1);
        }
        Surface::PlaneThrough { origin, u_point, v_point } if inside => {
            let o = DVec3::from_array(origin);
            return o + (DVec3::from_array(u_point) - o) * middle(0) + (DVec3::from_array(v_point) - o) * middle(1);
        }
        _ => {}
    }
    if inside {
        let net = f.nurbs.as_ref().unwrap_or(&f.surface);
        let mean = |ps: &mut dyn Iterator<Item = DVec3>| {
            let (sum, n) = ps.fold((DVec3::ZERO, 0usize), |(s, n), p| (s + p, n + 1));
            (n > 0).then(|| sum / n as f64)
        };
        let m = match net {
            Surface::Spline { points, .. } => mean(&mut points.iter().flatten().map(|p| DVec3::from_array(*p))),
            Surface::RationalSpline { homogeneous, .. } => {
                mean(&mut homogeneous.iter().flatten().map(|h| DVec3::new(h[0], h[1], h[2]) / h[3].max(1e-300)))
            }
            Surface::Cylinder { frame, radius } => {
                let a = (f.domain[0][0] + f.domain[0][1]) / 2.0;
                let (x, y) = (DVec3::from_array(frame.x_axis), DVec3::from_array(frame.y_axis));
                let z = x.cross(y);
                let v = (f.domain[1][0] + f.domain[1][1]) / 2.0;
                Some(DVec3::from_array(frame.origin) + (x * a.cos() + y * a.sin()) * *radius + z * v)
            }
            _ => None,
        };
        if let Some(m) = m {
            return m;
        }
    }
    let mut sum = DVec3::ZERO;
    let mut n = 0;
    for l in &f.loops {
        if let Loop::Edges { edges } = l {
            for e in edges {
                for v in r.edges[e.edge as usize].vertices {
                    sum += DVec3::from_array(r.vertices[v as usize].point);
                    n += 1;
                }
            }
        }
    }
    if n > 0 { sum / n as f64 } else { DVec3::ZERO }
}

fn is_plane(s: &Surface) -> bool {
    matches!(s, Surface::Plane { .. } | Surface::PlaneThrough { .. })
}

/// The names of `r`'s faces (by record index), from the recipe it was built from: a profile's
/// caps and sides, else their record index.
///
/// A face stands on the side whose curve (in the profile's plane: (x, y) for an extrusion,
/// (radius, height) for a turned profile) holds all its vertices: two sides share a corner at
/// most, so a face with vertices at two places has one — but a chord and an arc between the
/// same two corners (a D): the side must also fit the face's surface, a plane on a straight
/// side (for a turned profile a horizontal one: its other straight sides turn into cylinders
/// and cones), a curved face on any other. A turned disc's vertices are all on its rim (one
/// place, the corner of two sides): the side nearest a point inside it, its plane's middle on
/// the axis, is its own.
pub fn leaf_faces(r: &Record, recipe: Option<&Primitive>) -> Vec<FaceName> {
    let free = || (0..r.faces.len() as u32).map(FaceName::Face).collect();
    let (corners, sweep): (Vec<profile::Corner>, Option<(f64, f64)>) = match recipe {
        Some(Primitive::Prism { origin, profile, height }) => {
            (profile.iter().map(|c| profile::Corner { p: c.p + origin.truncate(), ..*c }).collect(), Some((origin.z, *height)))
        }
        Some(Primitive::Revolution { origin, profile }) if origin.truncate() == DVec2::ZERO => {
            (profile.iter().map(|c| profile::Corner { p: c.p + DVec2::Y * origin.z, ..*c }).collect(), None)
        }
        _ => return free(),
    };
    let outline = profile::rounded(&corners);
    let n = outline.len();
    let extent = profile::bounds(&outline).map_or(1.0, |(lo, hi)| (hi - lo).length());
    let tol = 1e-6 * (1.0 + extent);
    // The sides that make a face: of some length, and off the axis for a turned profile.
    let sides: Vec<(profile::Side, FaceName)> = (0..n)
        .filter_map(|k| {
            let side = profile::Side::new(outline[k].p, outline[(k + 1) % n].p, outline[k].bulge);
            let on_axis = sweep.is_none() && side.a().x.abs() <= tol && side.b().x.abs() <= tol;
            (side.length() > tol && !on_axis).then(|| {
                let name = match SideName::of(&outline[k]) {
                    SideName::Side(id) => FaceName::Side(id),
                    SideName::Fillet(id) => FaceName::Fillet(id),
                };
                (side, name)
            })
        })
        .collect();
    // A point in the profile's plane.
    let flat = |q: DVec3| match sweep {
        Some(_) => q.truncate(),
        None => DVec2::new(q.truncate().length(), q.z),
    };
    (0..r.faces.len())
        .map(|i| {
            let plane = is_plane(&r.faces[i].surface);
            let vertices: Vec<DVec3> = vertices_of(r, i).collect();
            if let Some((z0, h)) = sweep
                && plane
                && let Some(z) = vertices.first().map(|v| v.z)
                && vertices.iter().all(|v| (v.z - z).abs() < tol)
            {
                if (z - z0).abs() < tol {
                    return FaceName::Bottom;
                } else if (z - z0 - h).abs() < tol {
                    return FaceName::Top;
                }
            }
            let fits = |s: &profile::Side| match (s, sweep) {
                (profile::Side::Line { .. }, Some(_)) => plane,
                (profile::Side::Line { a, b }, None) => plane == ((a.y - b.y).abs() <= tol),
                (profile::Side::Arc { .. }, _) => !plane,
            };
            let on: Vec<&(profile::Side, FaceName)> =
                sides.iter().filter(|(s, _)| fits(s) && vertices.iter().all(|v| s.holds(flat(*v), tol))).collect();
            let inside = flat(sample(r, i, true));
            let nearest = |list: &mut dyn Iterator<Item = &(profile::Side, FaceName)>| {
                list.min_by(|a, b| a.0.distance(inside).total_cmp(&b.0.distance(inside))).map(|s| s.1.clone())
            };
            let name = match on.as_slice() {
                [(_, name)] => Some(name.clone()),
                [] => nearest(&mut sides.iter()),
                several => nearest(&mut several.iter().copied()),
            };
            name.unwrap_or(FaceName::Face(i as u32))
        })
        .collect()
}

/// The vertices of face `i` of `r` (each once per edge end).
fn vertices_of(r: &Record, i: usize) -> impl Iterator<Item = DVec3> + '_ {
    r.faces[i]
        .loops
        .iter()
        .filter_map(|l| if let Loop::Edges { edges } = l { Some(edges) } else { None })
        .flatten()
        .flat_map(|e| r.edges[e.edge as usize].vertices.map(|v| DVec3::from_array(r.vertices[v as usize].point)))
}

/// The record edges of `r`, a node's children's result (`map`: its face map), that `name`
/// names: where a node's edges lie, of one child or between two.
pub fn edges_named(r: &Record, map: &FaceMap, name: &EdgeName) -> Vec<u32> {
    let mut faces_of: Vec<Vec<usize>> = vec![Vec::new(); r.edges.len()];
    for (fi, f) in r.faces.iter().enumerate() {
        for l in &f.loops {
            if let Loop::Edges { edges } = l {
                for e in edges {
                    let list = &mut faces_of[e.edge as usize];
                    if !list.contains(&fi) {
                        list.push(fi);
                    }
                }
            }
        }
    }
    (0..r.edges.len() as u32)
        .filter(|&e| match faces_of[e as usize].as_slice() {
            [x, y] => map.get(*x).zip(map.get(*y)).is_some_and(|(a, b)| name.matches(a, b)),
            _ => false,
        })
        .collect()
}

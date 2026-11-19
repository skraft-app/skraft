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

/// What a node does to its children's result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Blend {
    /// A round: the size its radius.
    Round,
    /// A chamfer at equal distances along both faces: the size the distance.
    Chamfer,
}

/// A node's operation: its kind, size and edges.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeOp {
    pub blend: Blend,
    pub size: f64,
    pub edges: Vec<NodeEdges>,
}

/// A body of a node's children, as the node task computes it: its id (for the names' paths),
/// sign and placement in its parent's axes, and what it is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TreeBody {
    pub id: u64,
    pub negative: bool,
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

/// A group's recipe named: each input's body id and names, positives then negatives, as the
/// recipe lists them. The recipe then gives its result's face map.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecipeNames {
    pub positives: Vec<(u64, InputNames)>,
    pub negatives: Vec<(u64, InputNames)>,
}

/// A cheap fingerprint of a record: its counts and its vertices. A face map is used only on
/// the solid it was made for (one with this fingerprint).
pub fn fingerprint(r: &Record) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (r.faces.len(), r.edges.len(), r.vertices.len()).hash(&mut h);
    for v in &r.vertices {
        v.point.map(f64::to_bits).hash(&mut h);
    }
    h.finish()
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
            TreeNode::Leaf { solid, .. } | TreeNode::Mapped { solid, .. } => *solid = f(*solid),
            TreeNode::Group { children, .. } => map_leaves(children, f),
        }
    }
}

// ---- a leaf's names --------------------------------------------------------------------------

/// A point standing for face `i` of `r`: its vertices' centroid, or for a curved face the mean
/// of its control points (on the side it bulges to, where the vertices are those of a chord).
fn sample(r: &Record, i: usize, curved: bool) -> DVec3 {
    let f = &r.faces[i];
    if curved {
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
/// caps and sides (each face matched to the side it stands on), else their record index.
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
    let sides: Vec<(DVec2, FaceName)> = (0..n)
        .map(|k| {
            let at = profile::Side::new(outline[k].p, outline[(k + 1) % n].p, outline[k].bulge).at(0.5);
            let name = match SideName::of(&outline[k]) {
                SideName::Side(id) => FaceName::Side(id),
                SideName::Fillet(id) => FaceName::Fillet(id),
            };
            (at, name)
        })
        .collect();
    let nearest = |q: DVec2| sides.iter().min_by(|a, b| a.0.distance(q).total_cmp(&b.0.distance(q))).map(|s| s.1.clone());
    (0..r.faces.len())
        .map(|i| {
            let plane = is_plane(&r.faces[i].surface);
            let name = match sweep {
                Some((z0, h)) => {
                    let p = sample(r, i, !plane);
                    let flat = plane && sample_flat(r, i);
                    if flat && (p.z - z0).abs() < 1e-6 {
                        Some(FaceName::Bottom)
                    } else if flat && (p.z - z0 - h).abs() < 1e-6 {
                        Some(FaceName::Top)
                    } else {
                        nearest(p.truncate())
                    }
                }
                // Turned: the profile is in the (radius, height) plane, the face's vertices
                // on its seam.
                None => {
                    let p = sample(r, i, false);
                    nearest(DVec2::new(p.truncate().length(), p.z))
                }
            };
            name.unwrap_or(FaceName::Face(i as u32))
        })
        .collect()
}

/// Is face `i` of `r` square to Z: all its vertices at one height?
fn sample_flat(r: &Record, i: usize) -> bool {
    let mut zs = r.faces[i]
        .loops
        .iter()
        .filter_map(|l| if let Loop::Edges { edges } = l { Some(edges) } else { None })
        .flatten()
        .flat_map(|e| r.edges[e.edge as usize].vertices.map(|v| r.vertices[v as usize].point[2]));
    let Some(z) = zs.next() else { return false };
    zs.all(|w| (w - z).abs() < 1e-6)
}

/// The record edges of `r` between its faces named `a` and `b` (`names`: its faces', as
/// [`leaf_faces`] gives them): where a node's edge of one leaf lies on that leaf.
pub fn edges_between(r: &Record, names: &[FaceName], a: &FaceName, b: &FaceName) -> Vec<u32> {
    let mut faces_of = vec![Vec::new(); r.edges.len()];
    for (fi, f) in r.faces.iter().enumerate() {
        for l in &f.loops {
            if let Loop::Edges { edges } = l {
                for e in edges {
                    faces_of[e.edge as usize].push(fi);
                }
            }
        }
    }
    let named = |fi: usize| names.get(fi);
    (0..r.edges.len() as u32)
        .filter(|&e| match faces_of[e as usize].as_slice() {
            [x, y] => (named(*x) == Some(a) && named(*y) == Some(b)) || (named(*x) == Some(b) && named(*y) == Some(a)),
            _ => false,
        })
        .collect()
}

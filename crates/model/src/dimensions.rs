//! A drawing's dimensions (docs/ui/drawing.md#dimensions, decision 0048): measured on the
//! model through a view, hooked on a part's vertices and edges — by the names of the faces
//! around them when the part's shape has names (a single body: its recipe's, a node's map),
//! else by the point picked, in the part's axes; one gone is an orphan.

use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};
use skraft_kernel::names::{FaceMap, FaceTag};
use skraft_kernel::topo::EdgeShape;

use crate::{Document, Id, Part, PlacedPart, Subject};

/// A dimension on a sheet, measuring through view `view`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dimension {
    pub view: Id,
    pub kind: DimKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimKind {
    /// The distance between two points, along the sheet's x (horizontal), y (vertical) or
    /// between them (aligned); its line `offset` millimetres of paper off the line through
    /// them (on the side of the normal turned left from `a` to `b`).
    Linear { a: Anchor, b: Anchor, orient: Orient, offset: f64 },
    /// A circular edge's radius (or diameter), its text at `at` millimetres of paper from the
    /// circle's centre on the sheet.
    Radius { edge: Anchor, diameter: bool, at: DVec2 },
    /// The angle between two straight edges as the view shows them, its arc through the point
    /// `at` millimetres of paper from where they meet (which sector, how far).
    Angle { a: Anchor, b: Anchor, at: DVec2 },
}

/// Along what a linear dimension measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orient {
    Horizontal,
    Vertical,
    Aligned,
}

/// What an anchor points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Entity {
    Vertex,
    Edge,
}

/// A vertex or an edge of a part placed in a view's subject.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    /// The part placed: its occurrences from the subject down (`PlacedPart::path`'s chain).
    pub path: Vec<Id>,
    /// Its part (component).
    pub part: Id,
    pub entity: Entity,
    /// The names of the faces around it (each face's tags), when the part's shape has them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<Vec<FaceTag>>,
    /// The point picked (the vertex, a point of the edge), in the part's axes.
    pub at: DVec3,
}

/// An anchor found again, in the view's subject's axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Found {
    Point(DVec3),
    Edge(EdgeShape),
}

impl Found {
    /// A point of it: the vertex, an edge's middle (a circle's centre).
    pub fn point(&self) -> DVec3 {
        match *self {
            Found::Point(p) => p,
            Found::Edge(EdgeShape::Circle { centre, .. }) => centre,
            Found::Edge(e) => {
                let (a, b) = e.ends();
                (a + b) / 2.0
            }
        }
    }
}

/// A placed part's chain of occurrences.
fn chain(p: &PlacedPart) -> Vec<Id> {
    p.path.occurrences.iter().copied().chain([p.path.entity]).collect()
}

/// The names of `part`'s faces by record index, when its shape has them: a single positive
/// body, a leaf (named by its recipe) or a node (its map).
pub fn part_names(part: &Part) -> Option<FaceMap> {
    if !part.is_one_body() {
        return None;
    }
    let b = &part.bodies[0];
    let def = part.def_of(b);
    match (def.face_map(), &def.group) {
        (Some(m), _) => Some(m.iter().map(|tags| tags.iter().map(|t| skraft_kernel::names::under(&[b.id.0], t)).collect()).collect()),
        (None, None) => {
            Some(skraft_kernel::names::leaf_map(def.solid.record(), def.profile.as_ref().map(|p| p.primitive()).as_ref(), b.id.0))
        }
        (None, Some(_)) => None,
    }
}

/// Do the faces `cand` (each its tags) carry every face of `names` (one tag of each, at least)?
fn carries(names: &[Vec<FaceTag>], cand: &[Vec<FaceTag>]) -> bool {
    names.iter().all(|n| cand.iter().any(|c| c.iter().any(|t| n.contains(t))))
}

impl Document {
    /// An anchor on vertex `v` (or edge `e`, `entity` saying which) of `placed`'s part, `at`
    /// (in the part's axes) the point picked.
    pub fn anchor(&self, placed: &PlacedPart, entity: Entity, index: u32, at: DVec3) -> Option<Anchor> {
        let part = self.part(placed.part).ok()?;
        let shape = part.shape();
        let faces = match entity {
            Entity::Vertex => shape.vertex_faces(index),
            Entity::Edge => shape.edge_faces(index),
        };
        let names = part_names(part).map_or_else(Vec::new, |map| faces.iter().filter_map(|f| map.get(*f as usize).cloned()).collect());
        Some(Anchor { path: chain(placed), part: placed.part, entity, names, at })
    }

    /// Anchor `a` found again in `subject` as the model is now (its subject's axes): the vertex
    /// or edge of its names nearest its point, else (no names) the nearest of its kind within
    /// a hundredth of the part's size; none: an orphan.
    pub fn find(&self, subject: &Subject, a: &Anchor) -> Option<Found> {
        let placed = self.subject_parts(subject).into_iter().find(|p| p.part == a.part && chain(p) == a.path)?;
        let part = self.part(a.part).ok()?;
        let shape = part.shape();
        let points = shape.vertex_points();
        let (lo, hi) = points.iter().fold((DVec3::MAX, DVec3::MIN), |(l, h), p| (l.min(*p), h.max(*p)));
        let tolerance = 0.1 + 0.01 * (hi - lo).length().max(0.0);
        let map = if a.names.is_empty() { None } else { part_names(part) };
        let names_of = |faces: &[u32]| map.as_ref().map(|m| faces.iter().filter_map(|f| m.get(*f as usize).cloned()).collect::<Vec<_>>());
        let t = placed.transform;
        match a.entity {
            Entity::Vertex => {
                let best = (0..points.len() as u32)
                    .filter(|&v| match names_of(&shape.vertex_faces(v)) {
                        Some(cand) => carries(&a.names, &cand),
                        None => true,
                    })
                    .map(|v| (v, points[v as usize].distance(a.at)))
                    .min_by(|x, y| x.1.total_cmp(&y.1))?;
                (map.is_some() || best.1 <= tolerance).then(|| Found::Point(t.transform_point3(points[best.0 as usize])))
            }
            Entity::Edge => {
                let near = |s: &EdgeShape| match *s {
                    EdgeShape::Circle { centre, normal, radius, .. } => {
                        let d = a.at - centre;
                        let along = d.dot(normal);
                        ((d - normal * along).length() - radius).hypot(along)
                    }
                    EdgeShape::Line { a: p, b: q } | EdgeShape::Other { a: p, b: q } => {
                        let ab = q - p;
                        let k = ((a.at - p).dot(ab) / ab.length_squared().max(1e-300)).clamp(0.0, 1.0);
                        (p + ab * k).distance(a.at)
                    }
                };
                let best = (0..shape.edge_count() as u32)
                    .filter(|&e| match names_of(&shape.edge_faces(e)) {
                        Some(cand) => carries(&a.names, &cand),
                        None => true,
                    })
                    .filter_map(|e| shape.edge_shape(e).map(|s| (near(&s), s)))
                    .min_by(|x, y| x.0.total_cmp(&y.0))?;
                (map.is_some() || best.0 <= tolerance).then(|| Found::Edge(placed_shape(best.1, &t)))
            }
        }
    }
}

/// An edge's shape placed by `t` (rigid, or a mirror).
fn placed_shape(s: EdgeShape, t: &glam::DAffine3) -> EdgeShape {
    let p = |v: DVec3| t.transform_point3(v);
    match s {
        EdgeShape::Line { a, b } => EdgeShape::Line { a: p(a), b: p(b) },
        EdgeShape::Other { a, b } => EdgeShape::Other { a: p(a), b: p(b) },
        EdgeShape::Circle { centre, normal, radius, a, b } => {
            EdgeShape::Circle { centre: p(centre), normal: t.transform_vector3(normal).normalize_or_zero(), radius, a: p(a), b: p(b) }
        }
    }
}

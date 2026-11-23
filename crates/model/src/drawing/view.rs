//! A view of the model on a sheet (docs/ui/drawing.md#views): what it shows, from where, in
//! what style, and the axes it looks along.

use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};

use super::is_default;
use crate::{EntityPath, Id};

/// A view of the model on a sheet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub name: String,
    pub kind: ViewKind,
    #[serde(default, skip_serializing_if = "is_default")]
    pub style: ViewStyle,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewKind {
    /// A view of its own: what it shows, from where, at what scale; the centre of the
    /// subject's box seen there is drawn at `at`.
    Base { subject: Subject, look: Look, scale: f64, at: DVec2 },
    /// A view of a base view's subject, at its scale, seen from the side `side` says and
    /// aligned with it, `distance` from it (between their centres) on the sheet.
    Projected { base: Id, side: Side, distance: f64 },
}

/// What a view shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Subject {
    /// The whole model.
    Model,
    /// A component in its own axes.
    Component(Id),
    /// These occurrences, where they stand in the model.
    Occurrences(Vec<EntityPath>),
}

/// From where a base view looks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Look {
    /// A standard direction of the model's axes (or the component's).
    Standard(Direction),
    /// Any direction, parallel or in perspective (the 3D camera's when the view was made).
    Camera { forward: DVec3, up: DVec3, perspective: Option<Perspective> },
}

impl Look {
    /// The axes it looks along, and its perspective if any.
    pub fn frame(&self) -> (Frame, Option<Perspective>) {
        match self {
            Look::Standard(d) => (Frame::of(*d), None),
            Look::Camera { forward, up, perspective } => (Frame::looking(*forward, *up), *perspective),
        }
    }
}

/// A perspective view's eye, its opening across its frame's height, and that frame on the paper
/// (millimetres): the subject's box means nothing with the eye inside the model (decision 0051).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Perspective {
    pub eye: DVec3,
    pub fov_y: f64,
    pub size: DVec2,
}

impl Perspective {
    /// The widest opening kept when a frame grows.
    const MAX_HALF_FOV: f64 = 1.45;

    /// Its frame cropped to `size`, what it shows staying the same size on the paper.
    pub fn cropped(self, size: DVec2) -> Perspective {
        let half = ((self.fov_y / 2.0).tan() * size.y / self.size.y).atan().min(Self::MAX_HALF_FOV);
        Perspective { fov_y: 2.0 * half, size, ..self }
    }
}

impl super::Drawing {
    /// Does a view stand projected right from view `id`?
    pub fn has_projected_views(&self, id: Id) -> bool {
        self.container_of(id).and_then(|c| self.elements_in(c)).is_some_and(|list| {
            list.iter()
                .any(|el| matches!(&el.kind, super::ElementKind::View(View { kind: ViewKind::Projected { base, .. }, .. }) if *base == id))
        })
    }
}

/// A standard direction, as the 3D view's standard views (Z up).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
    Iso,
}

/// The side of its base a projected view goes to, on the sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Left,
    Right,
    Above,
    Below,
}

/// Where projected views go: first-angle (European: the view from the left goes right) or
/// third-angle (American: the view from the left goes left).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Projection {
    #[default]
    FirstAngle,
    ThirdAngle,
}

/// How a view is drawn (decision 0048).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewStyle {
    /// The visible and hidden edges, as vectors.
    #[default]
    Lines,
    /// An image of the 3D renderer.
    Rendered,
    /// The image with the lines over it.
    Hybrid,
}

/// The axes a view looks along, in its subject's axes: `forward` from the eye into the scene,
/// `up` and `right` the sheet's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub right: DVec3,
    pub up: DVec3,
    pub forward: DVec3,
}

impl Frame {
    /// The frame looking along `forward`, `up` as near up as it can be.
    pub fn looking(forward: DVec3, up: DVec3) -> Frame {
        let forward = forward.normalize();
        let right = forward.cross(up).try_normalize().unwrap_or_else(|| forward.any_orthonormal_vector());
        Frame { right, up: right.cross(forward), forward }
    }

    pub fn of(direction: Direction) -> Frame {
        let f = match direction {
            Direction::Front => DVec3::Y,
            Direction::Back => -DVec3::Y,
            Direction::Left => DVec3::X,
            Direction::Right => -DVec3::X,
            Direction::Top => -DVec3::Z,
            Direction::Bottom => DVec3::Z,
            Direction::Iso => DVec3::new(-1.0, 1.0, -1.0),
        };
        let up = match direction {
            Direction::Top => DVec3::Y,
            Direction::Bottom => -DVec3::Y,
            _ => DVec3::Z,
        };
        Frame::looking(f, up)
    }

    /// The frame of the view projected from this one to `side`.
    pub fn projected(self, side: Side, projection: Projection) -> Frame {
        // Third-angle: the view placed right looks from the right; first-angle from the left.
        let s = match projection {
            Projection::ThirdAngle => 1.0,
            Projection::FirstAngle => -1.0,
        };
        // Either way, the base view's near side faces it (its up or right the base's forward).
        let (forward, up) = match side {
            Side::Right => (-self.right * s, self.up),
            Side::Left => (self.right * s, self.up),
            Side::Above => (-self.up * s, self.forward),
            Side::Below => (self.up * s, -self.forward),
        };
        Frame::looking(forward, up)
    }

    /// A point seen in this frame: sheet x, y (model units) and depth.
    pub fn project(&self, p: DVec3) -> DVec3 {
        DVec3::new(p.dot(self.right), p.dot(self.up), p.dot(self.forward))
    }
}

impl Side {
    /// Its direction on the sheet.
    pub fn dir(self) -> DVec2 {
        match self {
            Side::Left => -DVec2::X,
            Side::Right => DVec2::X,
            Side::Above => DVec2::Y,
            Side::Below => -DVec2::Y,
        }
    }
}

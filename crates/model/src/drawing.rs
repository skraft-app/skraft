//! The document's drawing (docs/ui/drawing.md, decision 0048): sheets of paper holding views
//! of the model and free elements, on layers — a shared layer's elements on every sheet (its
//! title block). Changed by operations, as the rest of the document.

use glam::{DAffine3, DVec2, DVec3};
use serde::{Deserialize, Serialize};
use skraft_i18n::t;

use crate::{ComponentKind, Document, EntityPath, Error, Id, PlacedPart, Refusal, Result};

/// The sheets, in their order, and the layers their elements are on, bottom first.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Drawing {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sheets: Vec<Sheet>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<Layer>,
    /// Where projected views go (docs/ui/drawing.md#projected-views).
    #[serde(default, skip_serializing_if = "is_default")]
    pub projection: Projection,
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// A layer. A shared one holds its elements itself, drawn on every sheet (a title block); the
/// others' elements are each sheet's own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: Id,
    pub name: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shared: bool,
    /// Not drawn (nor printed).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// Drawn, but its elements neither picked nor changed on the sheet.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub elements: Vec<Element>,
}

/// A sheet of paper: its size and its own elements, in the order they are drawn.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub id: Id,
    pub name: String,
    pub paper: Paper,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub elements: Vec<Element>,
    /// Its grid's step, paper millimetres, the points drawn snap to when nothing else is near
    /// (docs/ui/drawing.md#snapping); 0: none.
    #[serde(default, skip_serializing_if = "no_grid")]
    pub grid: f64,
}

fn no_grid(g: &f64) -> bool {
    *g <= 0.0
}

/// A paper's size in millimetres, landscape when wider than high.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Paper {
    pub width: f64,
    pub height: f64,
}

impl Paper {
    /// ISO 216 A sizes, landscape.
    pub const A4: Paper = Paper { width: 297.0, height: 210.0 };
    pub const A3: Paper = Paper { width: 420.0, height: 297.0 };
    pub const A2: Paper = Paper { width: 594.0, height: 420.0 };
    pub const A1: Paper = Paper { width: 841.0, height: 594.0 };
    pub const A0: Paper = Paper { width: 1189.0, height: 841.0 };

    pub fn size(self) -> DVec2 {
        DVec2::new(self.width, self.height)
    }

    fn is_valid(self) -> bool {
        (1.0..=10_000.0).contains(&self.width) && (1.0..=10_000.0).contains(&self.height)
    }
}

/// Something on a sheet, on a layer. Sheet coordinates are millimetres of paper, from the
/// sheet's bottom left corner, y up.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Element {
    pub id: Id,
    pub layer: Id,
    pub kind: ElementKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElementKind {
    View(View),
    /// A text, its first line's baseline starting at `at`; `{…}` fields filled when drawn.
    Text {
        at: DVec2,
        text: String,
        size: f64,
    },
    Line {
        from: DVec2,
        to: DVec2,
    },
    Rect {
        min: DVec2,
        max: DVec2,
    },
    /// An image of the document's assets (`Document::assets`), stretched over the rectangle.
    Image {
        asset: String,
        min: DVec2,
        max: DVec2,
    },
    /// A dimension measured through a view (docs/ui/drawing.md#dimensions).
    Dimension(crate::Dimension),
}

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

/// A perspective view's eye and opening.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Perspective {
    pub eye: DVec3,
    pub fov_y: f64,
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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

/// A view as it is drawn: its own, or its base's for a projected one.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved<'a> {
    pub subject: &'a Subject,
    pub frame: Frame,
    pub perspective: Option<Perspective>,
    pub scale: f64,
    /// Where the centre of the subject's box seen in `frame` is drawn on the sheet.
    pub at: DVec2,
}

/// The most nested projections (a projected view of a projected view…).
const MAX_CHAIN: usize = 16;

impl Drawing {
    pub fn is_empty(&self) -> bool {
        self.sheets.is_empty() && self.layers.is_empty() && self.projection == Projection::default()
    }

    pub fn sheet(&self, id: Id) -> Option<&Sheet> {
        self.sheets.iter().find(|s| s.id == id)
    }

    pub fn layer(&self, id: Id) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    /// What sheet `sheet` shows, bottom first: the layers in their order, a shared layer's
    /// elements and the sheet's own on the others.
    pub fn elements_of<'a>(&'a self, sheet: &'a Sheet) -> impl Iterator<Item = &'a Element> {
        self.layers.iter().flat_map(move |l| {
            let (shared, own): (&[Element], &[Element]) = if l.shared { (&l.elements, &[]) } else { (&[], &sheet.elements) };
            shared.iter().chain(own.iter().filter(move |e| e.layer == l.id))
        })
    }

    /// The element `id`, on a sheet or a shared layer.
    pub fn element(&self, id: Id) -> Option<&Element> {
        self.sheets.iter().flat_map(|s| &s.elements).chain(self.layers.iter().flat_map(|l| &l.elements)).find(|e| e.id == id)
    }

    /// The element `id`, to change (a preview: the document's change through operations).
    pub fn element_mut(&mut self, id: Id) -> Option<&mut Element> {
        self.elements_mut().flat_map(|l| l.iter_mut()).find(|e| e.id == id)
    }

    fn view(&self, id: Id) -> Option<&View> {
        match &self.element(id)?.kind {
            ElementKind::View(v) => Some(v),
            _ => None,
        }
    }

    /// How the view `id` is drawn, following its bases; `None` when one is gone or the chain
    /// loops.
    pub fn resolve(&self, id: Id) -> Option<Resolved<'_>> {
        let mut chain = Vec::new();
        let mut view = self.view(id)?;
        loop {
            match &view.kind {
                ViewKind::Base { subject, look, scale, at } => {
                    let (mut frame, perspective) = look.frame();
                    let mut at = *at;
                    for (side, distance) in chain.into_iter().rev() {
                        frame = frame.projected(side, self.projection);
                        at += Side::dir(side) * distance;
                    }
                    return Some(Resolved { subject, frame, perspective, scale: *scale, at });
                }
                ViewKind::Projected { base, side, distance } => {
                    if chain.len() >= MAX_CHAIN {
                        return None;
                    }
                    chain.push((*side, *distance));
                    view = self.view(*base)?;
                }
            }
        }
    }

    /// What hangs on view `id`, at any depth: the views projected from it, the dimensions
    /// measured through them.
    pub fn projections_of(&self, id: Id) -> Vec<Id> {
        let views: Vec<(Id, Id)> = self
            .sheets
            .iter()
            .flat_map(|s| &s.elements)
            .chain(self.layers.iter().flat_map(|l| &l.elements))
            .filter_map(|e| match &e.kind {
                ElementKind::View(View { kind: ViewKind::Projected { base, .. }, .. }) => Some((e.id, *base)),
                ElementKind::Dimension(d) => Some((e.id, d.view)),
                _ => None,
            })
            .collect();
        let mut out = Vec::new();
        let mut todo = vec![id];
        while let Some(b) = todo.pop() {
            for (v, base) in &views {
                if *base == b && !out.contains(v) {
                    out.push(*v);
                    todo.push(*v);
                }
            }
        }
        out
    }

    /// The images its elements show.
    pub fn images(&self) -> impl Iterator<Item = &str> {
        self.sheets.iter().flat_map(|s| &s.elements).chain(self.layers.iter().flat_map(|l| &l.elements)).filter_map(|e| match &e.kind {
            ElementKind::Image { asset, .. } => Some(asset.as_str()),
            _ => None,
        })
    }

    fn elements_mut(&mut self) -> impl Iterator<Item = &mut Vec<Element>> {
        self.sheets.iter_mut().map(|s| &mut s.elements).chain(self.layers.iter_mut().map(|l| &mut l.elements))
    }
}

// ---- Operations ------------------------------------------------------------------------

/// Where an element added goes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placing {
    pub sheet: Id,
    /// Its layer: none, the first layer not shared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<Id>,
}

impl Document {
    pub fn drawing(&self) -> &Drawing {
        &self.drawing
    }

    /// The parts a view of `subject` shows, in the subject's axes, the hidden ones left out.
    pub fn subject_parts(&self, subject: &Subject) -> Vec<PlacedPart> {
        let mut out = match subject {
            Subject::Model => self.placed_parts(),
            Subject::Component(c) => match self.component(*c).map(|c| &c.kind) {
                Ok(ComponentKind::Part(p)) => vec![PlacedPart {
                    path: EntityPath::root(*c),
                    part: *c,
                    transform: DAffine3::IDENTITY,
                    shape: p.shape().clone(),
                    material: None,
                    hidden_at: None,
                }],
                Ok(ComponentKind::Assembly(_)) => {
                    let mut out = Vec::new();
                    self.walk(*c, &mut Vec::new(), DAffine3::IDENTITY, (None, None), &mut out, 0);
                    out
                }
                Err(_) => Vec::new(),
            },
            Subject::Occurrences(paths) => {
                let chains: Vec<Vec<Id>> = paths.iter().map(|p| p.instances.iter().copied().chain([p.entity]).collect()).collect();
                let mut all = self.placed_parts();
                all.retain(|p| {
                    let chain: Vec<Id> = p.path.instances.iter().copied().chain([p.path.entity]).collect();
                    chains.iter().any(|c| chain.starts_with(c))
                });
                all
            }
        };
        out.retain(|p| p.hidden_at.is_none());
        out
    }

    fn drawing_mut(&mut self) -> &mut Drawing {
        std::sync::Arc::make_mut(&mut self.drawing)
    }

    /// The layers a first sheet comes with: the drawing's, not shared, and the title block's,
    /// shared by every sheet.
    fn ensure_layers(&mut self) {
        if self.drawing.layers.is_empty() {
            let (plan, title) = (self.fresh_id(), self.fresh_id());
            let d = self.drawing_mut();
            d.layers.push(Layer {
                id: plan,
                name: t!("drawing.layer.plan"),
                shared: false,
                hidden: false,
                locked: false,
                elements: Vec::new(),
            });
            d.layers.push(Layer {
                id: title,
                name: t!("drawing.layer.title_block"),
                shared: true,
                hidden: false,
                locked: false,
                elements: Vec::new(),
            });
        }
    }

    pub(crate) fn add_sheet(&mut self, name: Option<&str>, paper: Paper) -> Result<Id> {
        if !paper.is_valid() {
            return Err(Error::Invalid(Refusal::BadPaper));
        }
        self.ensure_layers();
        let id = self.fresh_id();
        let n = self.drawing.sheets.len() + 1;
        let name = name.map(str::to_owned).unwrap_or_else(|| t!("drawing.sheet_name", n = n));
        self.drawing_mut().sheets.push(Sheet { id, name, paper, elements: Vec::new(), grid: 0.0 });
        Ok(id)
    }

    pub(crate) fn set_sheet(&mut self, sheet: Id, name: &str, paper: Paper, grid: f64) -> Result<()> {
        if !paper.is_valid() || !(0.0..=100.0).contains(&grid) {
            return Err(Error::Invalid(Refusal::BadPaper));
        }
        let s = self.drawing_mut().sheets.iter_mut().find(|s| s.id == sheet).ok_or(Error::NotFound(sheet))?;
        (s.name, s.paper, s.grid) = (name.to_owned(), paper, grid);
        Ok(())
    }

    pub(crate) fn delete_sheet(&mut self, sheet: Id) -> Result<()> {
        let d = self.drawing_mut();
        let i = d.sheets.iter().position(|s| s.id == sheet).ok_or(Error::NotFound(sheet))?;
        let gone: Vec<Id> = d.sheets[i].elements.iter().map(|e| e.id).collect();
        d.sheets.remove(i);
        // Views projected from one of its views, on another sheet, go with it.
        let orphans: Vec<Id> = gone.iter().flat_map(|v| d.projections_of(*v)).collect();
        for list in d.elements_mut() {
            list.retain(|e| !orphans.contains(&e.id));
        }
        self.prune_assets();
        Ok(())
    }

    pub(crate) fn move_sheet(&mut self, sheet: Id, index: usize) -> Result<()> {
        let d = self.drawing_mut();
        let i = d.sheets.iter().position(|s| s.id == sheet).ok_or(Error::NotFound(sheet))?;
        let s = d.sheets.remove(i);
        d.sheets.insert(index.min(d.sheets.len()), s);
        Ok(())
    }

    fn check_element(&self, kind: &ElementKind, own: Option<Id>) -> Result<()> {
        let finite = |v: DVec2| v.is_finite();
        let ok = match kind {
            ElementKind::View(v) => match &v.kind {
                ViewKind::Base { subject, look, scale, at } => {
                    let look_ok = match look {
                        Look::Standard(_) => true,
                        Look::Camera { forward, up, perspective } => {
                            forward.is_finite()
                                && up.is_finite()
                                && forward.length() > 1e-9
                                && perspective.is_none_or(|p| p.eye.is_finite() && p.fov_y > 0.0 && p.fov_y < 3.0)
                        }
                    };
                    let subject_ok = match subject {
                        Subject::Model => true,
                        Subject::Component(c) => self.components.contains_key(c),
                        Subject::Occurrences(paths) => !paths.is_empty(),
                    };
                    look_ok && subject_ok && scale.is_finite() && *scale > 0.0 && finite(*at)
                }
                ViewKind::Projected { base, distance, .. } => {
                    if Some(*base) == own || own.is_some_and(|o| self.drawing.projections_of(o).contains(base)) {
                        return Err(Error::Invalid(Refusal::ProjectionLoop));
                    }
                    match self.drawing.resolve(*base) {
                        None => return Err(Error::NotFound(*base)),
                        Some(r) if r.perspective.is_some() => return Err(Error::Invalid(Refusal::ProjectedFromPerspective)),
                        Some(_) => distance.is_finite(),
                    }
                }
            },
            ElementKind::Text { at, size, .. } => finite(*at) && size.is_finite() && *size > 0.0,
            ElementKind::Line { from, to } => finite(*from) && finite(*to),
            ElementKind::Rect { min, max } => finite(*min) && finite(*max),
            ElementKind::Image { asset, min, max } => self.assets.contains_key(asset) && finite(*min) && finite(*max),
            ElementKind::Dimension(dim) => {
                let anchor_ok = |a: &crate::Anchor| a.at.is_finite();
                matches!(self.drawing.element(dim.view).map(|e| &e.kind), Some(ElementKind::View(_)))
                    && match &dim.kind {
                        crate::DimKind::Linear { a, b, offset, .. } => anchor_ok(a) && anchor_ok(b) && offset.is_finite(),
                        crate::DimKind::Radius { edge, at, .. } => anchor_ok(edge) && finite(*at),
                        crate::DimKind::Angle { a, b, at } => anchor_ok(a) && anchor_ok(b) && finite(*at),
                    }
            }
        };
        if ok { Ok(()) } else { Err(Error::Invalid(Refusal::BadElement)) }
    }

    pub(crate) fn add_element(&mut self, at: &Placing, kind: &ElementKind) -> Result<Id> {
        self.drawing.sheet(at.sheet).ok_or(Error::NotFound(at.sheet))?;
        self.check_element(kind, None)?;
        let layer = match at.layer {
            Some(l) => self.drawing.layer(l).ok_or(Error::NotFound(l))?.id,
            None => self.drawing.layers.iter().find(|l| !l.shared).ok_or(Error::Invalid(Refusal::BadElement))?.id,
        };
        let id = self.fresh_id();
        let e = Element { id, layer, kind: kind.clone() };
        let d = self.drawing_mut();
        if d.layer(layer).is_some_and(|l| l.shared) {
            d.layers.iter_mut().find(|l| l.id == layer).unwrap().elements.push(e);
        } else {
            d.sheets.iter_mut().find(|s| s.id == at.sheet).unwrap().elements.push(e);
        }
        Ok(id)
    }

    pub(crate) fn set_element(&mut self, id: Id, kind: &ElementKind) -> Result<()> {
        let old = self.drawing.element(id).ok_or(Error::NotFound(id))?;
        if std::mem::discriminant(&old.kind) != std::mem::discriminant(kind) {
            return Err(Error::Invalid(Refusal::BadElement));
        }
        self.check_element(kind, Some(id))?;
        let e = self.drawing_mut().elements_mut().flat_map(|l| l.iter_mut()).find(|e| e.id == id).unwrap();
        e.kind = kind.clone();
        self.prune_assets();
        Ok(())
    }

    /// Deletes the elements `ids`, and the views projected from them.
    pub(crate) fn delete_elements(&mut self, ids: &[Id]) -> Result<()> {
        if ids.is_empty() {
            return Err(Error::Invalid(Refusal::BadElement));
        }
        let mut gone = Vec::new();
        for id in ids {
            self.drawing.element(*id).ok_or(Error::NotFound(*id))?;
            gone.push(*id);
            gone.extend(self.drawing.projections_of(*id));
        }
        for list in self.drawing_mut().elements_mut() {
            list.retain(|e| !gone.contains(&e.id));
        }
        self.prune_assets();
        Ok(())
    }

    pub(crate) fn set_projection(&mut self, projection: Projection) {
        self.drawing_mut().projection = projection;
    }

    /// The sheet `sheet` as a template (docs/ui/drawing.md#templates): a document holding
    /// only a drawing — that sheet's paper and name, every layer, the shared ones with their
    /// elements but views (a template shows no model), the images they show.
    pub fn template_of(&self, sheet: Id) -> Result<Document> {
        let s = self.drawing.sheet(sheet).ok_or(Error::NotFound(sheet))?;
        let layers: Vec<Layer> = self
            .drawing
            .layers
            .iter()
            .map(|l| Layer {
                elements: if l.shared {
                    l.elements.iter().filter(|e| !matches!(e.kind, ElementKind::View(_))).cloned().collect()
                } else {
                    Vec::new()
                },
                ..l.clone()
            })
            .collect();
        let mut t = Document::new();
        t.next_id = self.next_id;
        t.drawing = std::sync::Arc::new(Drawing {
            sheets: vec![Sheet { id: s.id, name: s.name.clone(), paper: s.paper, elements: Vec::new(), grid: s.grid }],
            layers,
            projection: self.drawing.projection,
        });
        let images: Vec<String> = t.drawing.images().map(str::to_owned).collect();
        t.assets = self.assets.iter().filter(|(id, _)| images.contains(id)).map(|(k, v)| (k.clone(), v.clone())).collect();
        Ok(t)
    }

    /// The template Skraft comes with: A3 landscape, a frame 10 mm in, a title block at the
    /// bottom right — the document's name, the sheet's, the date, the scale, the sheet's
    /// number — on the shared layer « Cartouche ».
    pub fn default_template() -> Document {
        let mut t = Document::new();
        t.ensure_layers();
        let title = t.drawing.layers.iter().find(|l| l.shared).map(|l| l.id).unwrap_or(Id(0));
        let v = DVec2::new;
        let text = |at: DVec2, text: String, size: f64| ElementKind::Text { at, text, size };
        let kinds = vec![
            ElementKind::Rect { min: v(10.0, 10.0), max: v(410.0, 287.0) },
            ElementKind::Rect { min: v(250.0, 10.0), max: v(410.0, 46.0) },
            ElementKind::Line { from: v(250.0, 34.0), to: v(410.0, 34.0) },
            ElementKind::Line { from: v(250.0, 22.0), to: v(410.0, 22.0) },
            ElementKind::Line { from: v(340.0, 10.0), to: v(340.0, 34.0) },
            text(v(253.0, 37.5), "{document}".into(), 5.0),
            text(v(253.0, 25.5), t!("drawing.template.sheet"), 2.5),
            text(v(253.0, 13.5), t!("drawing.template.date"), 2.5),
            text(v(343.0, 25.5), t!("drawing.template.scale"), 2.5),
            text(v(343.0, 13.5), t!("drawing.template.number"), 2.5),
        ];
        let elements: Vec<Element> = kinds.into_iter().map(|kind| Element { id: t.fresh_id(), layer: title, kind }).collect();
        let sheet = t.fresh_id();
        let d = t.drawing_mut();
        if let Some(l) = d.layers.iter_mut().find(|l| l.id == title) {
            l.elements = elements;
        }
        d.sheets.push(Sheet { id: sheet, name: t!("drawing.template.default"), paper: Paper::A3, elements: Vec::new(), grid: 0.0 });
        t
    }

    /// A sheet added from `template` (docs/ui/drawing.md#templates), « Feuille n » unless
    /// named: its paper; its layers
    /// joined to the document's — one of the same name and sharing taken as it is (a shared
    /// one still empty filled with the template's elements), the others added with them —,
    /// their images taken in.
    pub(crate) fn add_sheet_from(&mut self, template: &Document, name: Option<&str>) -> Result<Id> {
        let tsheet = template.drawing.sheets.first();
        let paper = tsheet.map_or(Paper::A3, |s| s.paper);
        let mut images = Vec::new();
        for tl in &template.drawing.layers {
            let found =
                self.drawing.layers.iter().find(|l| l.name == tl.name && l.shared == tl.shared).map(|l| (l.id, l.elements.is_empty()));
            let (id, fill) = match found {
                Some((id, empty)) => (id, tl.shared && empty),
                None => {
                    let id = self.fresh_id();
                    self.drawing_mut().layers.push(Layer { id, elements: Vec::new(), ..tl.clone() });
                    (id, tl.shared)
                }
            };
            if fill {
                let copies: Vec<Element> = tl
                    .elements
                    .iter()
                    .filter(|e| !matches!(e.kind, ElementKind::View(_)))
                    .map(|e| Element { id: self.fresh_id(), layer: id, kind: e.kind.clone() })
                    .collect();
                for e in &copies {
                    if let ElementKind::Image { asset, .. } = &e.kind {
                        images.push(asset.clone());
                    }
                }
                if let Some(l) = self.drawing_mut().layers.iter_mut().find(|l| l.id == id) {
                    l.elements.extend(copies);
                }
            }
        }
        for a in images {
            if let Some(asset) = template.assets.get(&a) {
                self.assets.insert(a, asset.clone());
            }
        }
        // Something to draw on.
        self.ensure_layers();
        if !self.drawing.layers.iter().any(|l| !l.shared) {
            self.add_layer(Some(&t!("drawing.layer.plan")))?;
        }
        // « Feuille n » unless named: the template's own sheet name is the template's; its grid.
        let id = self.add_sheet(name, paper)?;
        let grid = tsheet.map_or(0.0, |s| s.grid);
        if let Some(s) = self.drawing_mut().sheets.iter_mut().find(|s| s.id == id) {
            s.grid = grid;
        }
        Ok(id)
    }

    /// A layer added on top of the others, not shared, named « Calque n » when `name` is none.
    pub(crate) fn add_layer(&mut self, name: Option<&str>) -> Result<Id> {
        self.ensure_layers();
        let id = self.fresh_id();
        let n = self.drawing.layers.len() + 1;
        let name = name.map(str::to_owned).unwrap_or_else(|| t!("drawing.layer.name", n = n));
        self.drawing_mut().layers.push(Layer { id, name, shared: false, hidden: false, locked: false, elements: Vec::new() });
        Ok(id)
    }

    pub(crate) fn set_layer(&mut self, layer: Id, name: &str, hidden: bool, locked: bool) -> Result<()> {
        let l = self.drawing_mut().layers.iter_mut().find(|l| l.id == layer).ok_or(Error::NotFound(layer))?;
        (l.name, l.hidden, l.locked) = (name.to_owned(), hidden, locked);
        Ok(())
    }

    /// A layer shared — its elements on `sheet` become every sheet's, those of the other sheets
    /// on it deleted (LayOut's) — or not — its elements copied onto every sheet.
    pub(crate) fn share_layer(&mut self, layer: Id, shared: bool, sheet: Id) -> Result<()> {
        let l = self.drawing.layer(layer).ok_or(Error::NotFound(layer))?;
        if l.shared == shared {
            return Ok(());
        }
        if shared {
            self.drawing.sheet(sheet).ok_or(Error::NotFound(sheet))?;
            // The other sheets' elements on it, and the views projected from them, deleted.
            let gone: Vec<Id> = self
                .drawing
                .sheets
                .iter()
                .filter(|s| s.id != sheet)
                .flat_map(|s| &s.elements)
                .filter(|e| e.layer == layer)
                .map(|e| e.id)
                .collect();
            if !gone.is_empty() {
                self.delete_elements(&gone)?;
            }
            let d = self.drawing_mut();
            let s = d.sheets.iter_mut().find(|s| s.id == sheet).unwrap();
            let (moved, kept): (Vec<Element>, Vec<Element>) = std::mem::take(&mut s.elements).into_iter().partition(|e| e.layer == layer);
            s.elements = kept;
            let l = d.layers.iter_mut().find(|l| l.id == layer).unwrap();
            l.elements = moved;
            l.shared = true;
        } else {
            // A copy on each sheet: the first keeps its ids, the others fresh ones (a view
            // projected from one of them follows its copy).
            let elements = self.drawing.layer(layer).map(|l| l.elements.clone()).unwrap_or_default();
            let sheets: Vec<Id> = self.drawing.sheets.iter().map(|s| s.id).collect();
            let mut copies = Vec::with_capacity(sheets.len());
            for (k, _) in sheets.iter().enumerate() {
                if k == 0 {
                    copies.push(elements.clone());
                    continue;
                }
                let map: std::collections::HashMap<Id, Id> = elements.iter().map(|e| (e.id, self.fresh_id())).collect();
                copies.push(
                    elements
                        .iter()
                        .map(|e| {
                            let mut c = e.clone();
                            c.id = map[&e.id];
                            if let ElementKind::View(View { kind: ViewKind::Projected { base, .. }, .. }) = &mut c.kind
                                && let Some(b) = map.get(base)
                            {
                                *base = *b;
                            }
                            c
                        })
                        .collect(),
                );
            }
            let d = self.drawing_mut();
            for (s, copy) in d.sheets.iter_mut().zip(copies) {
                s.elements.extend(copy);
            }
            let l = d.layers.iter_mut().find(|l| l.id == layer).unwrap();
            l.elements.clear();
            l.shared = false;
        }
        Ok(())
    }

    /// A layer deleted with its elements (and the views projected from them); a layer not
    /// shared stays (something to draw on).
    pub(crate) fn delete_layer(&mut self, layer: Id) -> Result<()> {
        let l = self.drawing.layer(layer).ok_or(Error::NotFound(layer))?;
        if !l.shared && self.drawing.layers.iter().filter(|l| !l.shared).count() < 2 {
            return Err(Error::Invalid(Refusal::LastLayer));
        }
        let gone: Vec<Id> =
            self.drawing.sheets.iter().flat_map(|s| &s.elements).chain(&l.elements).filter(|e| e.layer == layer).map(|e| e.id).collect();
        if !gone.is_empty() {
            self.delete_elements(&gone)?;
        }
        self.drawing_mut().layers.retain(|l| l.id != layer);
        Ok(())
    }

    pub(crate) fn move_layer(&mut self, layer: Id, index: usize) -> Result<()> {
        let d = self.drawing_mut();
        let i = d.layers.iter().position(|l| l.id == layer).ok_or(Error::NotFound(layer))?;
        let l = d.layers.remove(i);
        d.layers.insert(index.min(d.layers.len()), l);
        Ok(())
    }

    /// Elements moved onto `layer`: onto a shared one, every sheet's; off one, onto `sheet`.
    pub(crate) fn move_to_layer(&mut self, elements: &[Id], layer: Id, sheet: Id) -> Result<()> {
        let to = self.drawing.layer(layer).ok_or(Error::NotFound(layer))?.shared;
        self.drawing.sheet(sheet).ok_or(Error::NotFound(sheet))?;
        let d = self.drawing_mut();
        let mut taken = Vec::new();
        for list in d.elements_mut() {
            let (out, keep): (Vec<Element>, Vec<Element>) = std::mem::take(list).into_iter().partition(|e| elements.contains(&e.id));
            *list = keep;
            taken.extend(out);
        }
        if taken.len() != elements.len() {
            return Err(Error::Invalid(Refusal::BadElement));
        }
        for e in &mut taken {
            e.layer = layer;
        }
        if to {
            d.layers.iter_mut().find(|l| l.id == layer).unwrap().elements.extend(taken);
        } else {
            d.sheets.iter_mut().find(|s| s.id == sheet).unwrap().elements.extend(taken);
        }
        Ok(())
    }
}

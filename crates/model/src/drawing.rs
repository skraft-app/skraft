//! The document's drawing (docs/ui/drawing.md, decisions 0048 and 0049): a tree of presentations
//! holding sheets of paper holding elements — views of the model, free elements, occurrences of
//! the compositions, the definitions all presentations share. Changed by operations, as the rest of
//! the document.

mod clipboard;
mod ops;
mod template;
mod view;

use std::collections::{BTreeMap, BTreeSet};

use glam::{DAffine2, DVec2};
use serde::{Deserialize, Serialize};

pub use ops::FieldOn;
pub use view::{Crop, Direction, Frame, Look, Perspective, Projection, Side, Subject, View, ViewKind, ViewStyle};

use crate::{Id, Props};

/// The presentations, in their order, and the definitions they share.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Drawing {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presentations: Vec<Presentation>,
    /// The definitions, by id (docs/ui/drawing.md#compositions).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub compositions: BTreeMap<Id, Composition>,
}

pub(crate) fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

fn no_grid(g: &f64) -> bool {
    *g <= 0.0
}

/// A document of sheets for someone (« Atelier », « Client »): numbered, valued and exported on
/// its own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Presentation {
    pub id: Id,
    pub name: String,
    /// Where its projected views go.
    #[serde(default, skip_serializing_if = "is_default")]
    pub projection: Projection,
    /// The values its fields take (docs/ui/drawing.md#fields).
    #[serde(default, skip_serializing_if = "Props::is_empty")]
    pub props: Props,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sheets: Vec<Sheet>,
}

/// A sheet of paper: its size and its elements, in the order they are drawn.
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
    /// Its frame, drawn from its paper.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<Border>,
    /// The values its fields take.
    #[serde(default, skip_serializing_if = "Props::is_empty")]
    pub props: Props,
}

/// A sheet's frame: a rectangle `margin` millimetres inside its paper, `width` wide.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Border {
    pub margin: f64,
    pub width: f64,
}

impl Border {
    /// Well formed on `paper`: a line of 0.05 to 10 mm, leaving some paper inside.
    fn fits(self, paper: Paper) -> bool {
        self.margin.is_finite()
            && self.margin >= 0.0
            && 2.0 * self.margin < paper.width.min(paper.height)
            && self.width.is_finite()
            && (0.05..=10.0).contains(&self.width)
    }
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

/// A composition: elements in axes of its own, placed by occurrences; made from a selection with an
/// automatic name (decision 0049).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Composition {
    pub id: Id,
    pub name: String,
    /// Named by the user: it stays while placed nowhere.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub named: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub elements: Vec<Element>,
    /// The template's definition it comes from: brought in once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// Something on a sheet or in a composition. Coordinates are millimetres of paper in its container's
/// axes: a sheet's from its bottom left corner, y up.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Element {
    pub id: Id,
    /// Not drawn (nor printed), nor what it holds.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// Drawn, but neither picked nor changed on the sheet, nor what it holds.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub locked: bool,
    pub kind: ElementKind,
}

impl Element {
    /// An element shown and free.
    pub(crate) fn new(id: Id, kind: ElementKind) -> Self {
        Element { id, hidden: false, locked: false, kind }
    }
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
    Circle {
        center: DVec2,
        radius: f64,
    },
    /// An image of the document's assets (`Document::assets`), stretched over the rectangle.
    Image {
        asset: String,
        min: DVec2,
        max: DVec2,
    },
    /// A dimension measured through a view (docs/ui/drawing.md#dimensions).
    Dimension(crate::Dimension),
    /// A composition placed.
    Occurrence(Occurrence),
}

/// A composition placed in a sheet or another composition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Occurrence {
    pub composition: Id,
    pub place: Place,
    /// The values its fields take here, before the sheet's.
    #[serde(default, skip_serializing_if = "Props::is_empty")]
    pub props: Props,
}

/// Where an occurrence puts its composition's origin, and how it turns it: never a scale (a view at
/// 1:10 stays true).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Place {
    /// The paper's corner `at` is measured from, following the paper when its format changes.
    /// Only on a sheet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner: Option<Corner>,
    pub at: DVec2,
    /// Radians, counterclockwise, about the composition's origin.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rotation: f64,
}

impl Place {
    /// At `at` in its container.
    pub fn free(at: DVec2) -> Self {
        Place { corner: None, at, rotation: 0.0 }
    }

    /// At `offset` from a corner of the paper.
    pub fn anchored(corner: Corner, offset: DVec2) -> Self {
        Place { corner: Some(corner), at: offset, rotation: 0.0 }
    }

    /// Where the composition's origin goes, on `paper` when its container is a sheet.
    pub fn origin(&self, paper: Option<Paper>) -> DVec2 {
        match (self.corner, paper) {
            (Some(c), Some(p)) => c.on(p) + self.at,
            _ => self.at,
        }
    }

    /// The same place on `paper`, measured from `corner` (none: from the paper's origin).
    pub fn with_corner(self, corner: Option<Corner>, paper: Paper) -> Place {
        let origin = self.origin(Some(paper));
        Place { corner, at: origin - corner.map_or(DVec2::ZERO, |c| c.on(paper)), rotation: self.rotation }
    }

    /// From the composition's axes to its container's.
    pub fn transform(&self, paper: Option<Paper>) -> DAffine2 {
        DAffine2::from_angle_translation(self.rotation, self.origin(paper))
    }

    fn is_valid(&self) -> bool {
        self.at.is_finite() && self.rotation.is_finite()
    }
}

/// A corner of the paper.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Corner {
    BottomLeft,
    BottomRight,
    TopLeft,
    TopRight,
}

impl Corner {
    /// Where it is on `paper`.
    pub fn on(self, paper: Paper) -> DVec2 {
        match self {
            Corner::BottomLeft => DVec2::ZERO,
            Corner::BottomRight => DVec2::new(paper.width, 0.0),
            Corner::TopLeft => DVec2::new(0.0, paper.height),
            Corner::TopRight => paper.size(),
        }
    }
}

/// The names of the `{…}` fields of `text`, as written.
pub fn text_fields(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        let open = rest.find('{')?;
        let tail = &rest[open + 1..];
        let close = tail.find('}')?;
        rest = &tail[close + 1..];
        Some(&tail[..close])
    })
}

/// The property a field reads: `client` is the user's, `user.client`; a name with a namespace
/// is that key.
pub fn field_key(name: &str) -> String {
    if name.contains('.') { name.to_owned() } else { format!("user.{name}") }
}

/// What holds elements.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Container {
    Sheet(Id),
    Composition(Id),
}

/// An element as a sheet shows it, through the occurrences leading to it.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed<'a> {
    pub element: &'a Element,
    /// The occurrences from the sheet down to it, outermost first; empty on the sheet itself.
    pub path: Vec<Id>,
    /// From its container's axes to the sheet's.
    pub transform: DAffine2,
    /// It or an occurrence above it is hidden.
    pub hidden: bool,
    /// It or an occurrence above it is locked.
    pub locked: bool,
    /// Its presentation's: how a projected view in a composition is drawn here.
    pub projection: Projection,
}

impl Placed<'_> {
    /// What is right on the sheet for it: the outermost occurrence, else itself. What a click
    /// on the sheet picks.
    pub fn top(&self) -> Id {
        self.path.first().copied().unwrap_or(self.element.id)
    }

    /// What a click picks for it with the occurrences `open` entered: the element right in the
    /// last one, itself or the occurrence leading to it; none when it is not in there.
    pub fn under(&self, open: &[Id]) -> Option<Id> {
        let rest = self.path.strip_prefix(open)?;
        Some(rest.first().copied().unwrap_or(self.element.id))
    }
}

/// A view as it is drawn: its own, or its base's for a projected one.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved<'a> {
    pub subject: &'a Subject,
    pub frame: Frame,
    pub perspective: Option<Perspective>,
    pub scale: f64,
    /// Where the centre of the subject's box seen in `frame` is drawn in its container.
    pub at: DVec2,
    /// Its frame around `at`, when it is not the whole subject (a base view's alone).
    pub crop: Option<Crop>,
}

/// The most nested projections (a projected view of a projected view…).
const MAX_CHAIN: usize = 16;

impl Drawing {
    pub fn is_empty(&self) -> bool {
        self.presentations.is_empty() && self.compositions.is_empty()
    }

    pub fn presentation(&self, id: Id) -> Option<&Presentation> {
        self.presentations.iter().find(|p| p.id == id)
    }

    /// Every sheet, presentation by presentation.
    pub fn sheets(&self) -> impl Iterator<Item = &Sheet> {
        self.presentations.iter().flat_map(|p| &p.sheets)
    }

    pub fn sheet(&self, id: Id) -> Option<&Sheet> {
        self.sheets().find(|s| s.id == id)
    }

    /// The presentation sheet `id` is in.
    pub fn presentation_of(&self, sheet: Id) -> Option<&Presentation> {
        self.presentations.iter().find(|p| p.sheets.iter().any(|s| s.id == sheet))
    }

    /// Sheet `id`'s number in its presentation, from 1, and how many it has.
    pub fn sheet_number(&self, sheet: Id) -> Option<(usize, usize)> {
        let p = self.presentation_of(sheet)?;
        Some((p.sheets.iter().position(|s| s.id == sheet)? + 1, p.sheets.len()))
    }

    pub fn composition(&self, id: Id) -> Option<&Composition> {
        self.compositions.get(&id)
    }

    /// Everything holding elements, with them.
    pub(crate) fn containers(&self) -> impl Iterator<Item = (Container, &[Element])> {
        self.sheets()
            .map(|s| (Container::Sheet(s.id), s.elements.as_slice()))
            .chain(self.compositions.values().map(|b| (Container::Composition(b.id), b.elements.as_slice())))
    }

    pub(crate) fn lists_mut(&mut self) -> impl Iterator<Item = &mut Vec<Element>> {
        let Drawing { presentations, compositions } = self;
        presentations
            .iter_mut()
            .flat_map(|p| p.sheets.iter_mut().map(|s| &mut s.elements))
            .chain(compositions.values_mut().map(|b| &mut b.elements))
    }

    /// What `container` holds.
    pub fn elements_in(&self, container: Container) -> Option<&[Element]> {
        match container {
            Container::Sheet(s) => self.sheet(s).map(|s| s.elements.as_slice()),
            Container::Composition(b) => self.composition(b).map(|b| b.elements.as_slice()),
        }
    }

    fn elements_in_mut(&mut self, container: Container) -> Option<&mut Vec<Element>> {
        match container {
            Container::Sheet(id) => self.presentations.iter_mut().flat_map(|p| &mut p.sheets).find(|s| s.id == id).map(|s| &mut s.elements),
            Container::Composition(b) => self.compositions.get_mut(&b).map(|b| &mut b.elements),
        }
    }

    /// The container holding element `id`.
    pub fn container_of(&self, id: Id) -> Option<Container> {
        self.containers().find(|(_, list)| list.iter().any(|e| e.id == id)).map(|(c, _)| c)
    }

    /// The element `id`, on a sheet or in a composition.
    pub fn element(&self, id: Id) -> Option<&Element> {
        self.containers().flat_map(|(_, list)| list).find(|e| e.id == id)
    }

    /// The element `id`, to change (a preview: the document's change through operations).
    pub fn element_mut(&mut self, id: Id) -> Option<&mut Element> {
        self.lists_mut().flat_map(|l| l.iter_mut()).find(|e| e.id == id)
    }

    fn view(&self, id: Id) -> Option<&View> {
        match &self.element(id)?.kind {
            ElementKind::View(v) => Some(v),
            _ => None,
        }
    }

    /// The projection view `id` is drawn with: its presentation's, for one in a composition the
    /// first presentation placing it (a sheet's walk knows its own, [`Placed::projection`]).
    pub fn projection_of(&self, id: Id) -> Projection {
        match self.container_of(id) {
            Some(Container::Sheet(s)) => self.presentation_of(s).map_or_else(Projection::default, |p| p.projection),
            Some(Container::Composition(b)) => self
                .presentations
                .iter()
                .find(|p| p.sheets.iter().flat_map(|s| &s.elements).any(|e| self.reaches_element(e, b)))
                .map_or_else(Projection::default, |p| p.projection),
            None => Projection::default(),
        }
    }

    /// How the view `id` is drawn, following its bases; `None` when one is gone or the chain
    /// loops.
    pub fn resolve(&self, id: Id) -> Option<Resolved<'_>> {
        self.resolve_with(id, || self.projection_of(id))
    }

    /// How the view `id` is drawn in a presentation of projection `projection`.
    pub fn resolve_as(&self, id: Id, projection: Projection) -> Option<Resolved<'_>> {
        self.resolve_with(id, || projection)
    }

    fn resolve_with(&self, id: Id, projection: impl FnOnce() -> Projection) -> Option<Resolved<'_>> {
        let mut chain = Vec::new();
        let mut view = self.view(id)?;
        let crop = view.crop;
        loop {
            match &view.kind {
                ViewKind::Base { subject, look, scale, at } => {
                    let (mut frame, perspective) = look.frame();
                    let mut at = *at;
                    let crop = if chain.is_empty() { crop } else { None };
                    if !chain.is_empty() {
                        let projection = projection();
                        for (side, distance) in chain.into_iter().rev() {
                            frame = frame.projected(side, projection);
                            at += Side::dir(side) * distance;
                        }
                    }
                    return Some(Resolved { subject, frame, perspective, scale: *scale, at, crop });
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
    /// measured through them. They are all in its container.
    pub fn projections_of(&self, id: Id) -> Vec<Id> {
        let Some(list) = self.container_of(id).and_then(|c| self.elements_in(c)) else { return Vec::new() };
        let views: Vec<(Id, Id)> = list.iter().filter_map(|e| e.kind.hung_on().map(|base| (e.id, base))).collect();
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

    /// Its views, on sheets and in compositions.
    pub fn view_ids(&self) -> impl Iterator<Item = Id> + '_ {
        self.containers().flat_map(|(_, list)| list).filter(|e| matches!(e.kind, ElementKind::View(_))).map(|e| e.id)
    }

    /// The images its elements show.
    pub fn images(&self) -> impl Iterator<Item = &str> {
        self.containers().flat_map(|(_, list)| list).filter_map(|e| match &e.kind {
            ElementKind::Image { asset, .. } => Some(asset.as_str()),
            _ => None,
        })
    }

    /// The occurrences of composition `id`, wherever they are.
    pub fn occurrences_of(&self, composition: Id) -> Vec<Id> {
        self.containers()
            .flat_map(|(_, list)| list)
            .filter(|e| matches!(&e.kind, ElementKind::Occurrence(i) if i.composition == composition))
            .map(|e| e.id)
            .collect()
    }

    /// The names of the fields the texts of `list` hold, through its occurrences.
    pub fn field_names(&self, list: &[Element]) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        self.gather_fields(list, &mut Vec::new(), &mut out);
        out
    }

    fn gather_fields(&self, list: &[Element], seen: &mut Vec<Id>, out: &mut BTreeSet<String>) {
        for e in list {
            match &e.kind {
                ElementKind::Text { text, .. } => out.extend(text_fields(text).map(str::to_owned)),
                ElementKind::Occurrence(i) if !seen.contains(&i.composition) => {
                    seen.push(i.composition);
                    if let Some(b) = self.composition(i.composition) {
                        self.gather_fields(&b.elements, seen, out);
                    }
                }
                _ => {}
            }
        }
    }

    /// The names of the fields of every text of the drawing.
    pub fn all_field_names(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for (_, list) in self.containers() {
            for e in list {
                if let ElementKind::Text { text, .. } = &e.kind {
                    out.extend(text_fields(text).map(str::to_owned));
                }
            }
        }
        out
    }

    /// How many occurrences each composition has, wherever they are (a composition without any: none).
    pub fn composition_uses(&self) -> BTreeMap<Id, usize> {
        let mut uses = BTreeMap::new();
        for e in self.containers().flat_map(|(_, list)| list) {
            if let Some(i) = e.kind.occurrence() {
                *uses.entry(i.composition).or_insert(0) += 1;
            }
        }
        uses
    }

    /// How many times each composition shows on the sheets, those inside another counted once
    /// per occurrence of it. One that is not there is unused, even if an unused one places it.
    pub fn counts_on_sheets(&self) -> BTreeMap<Id, usize> {
        fn walk(d: &Drawing, list: &[Element], out: &mut BTreeMap<Id, usize>) {
            for i in list.iter().filter_map(|e| e.kind.occurrence()) {
                *out.entry(i.composition).or_insert(0) += 1;
                if let Some(c) = d.composition(i.composition) {
                    walk(d, &c.elements, out);
                }
            }
        }
        let mut out = BTreeMap::new();
        for s in self.sheets() {
            walk(self, &s.elements, &mut out);
        }
        out
    }

    /// Does composition `from` hold composition `to`, or is it `to`, at any depth?
    pub fn reaches(&self, from: Id, to: Id) -> bool {
        let mut seen = Vec::new();
        let mut todo = vec![from];
        while let Some(b) = todo.pop() {
            if b == to {
                return true;
            }
            if seen.contains(&b) {
                continue;
            }
            seen.push(b);
            if let Some(composition) = self.composition(b) {
                todo.extend(composition.elements.iter().filter_map(|e| e.kind.occurrence().map(|i| i.composition)));
            }
        }
        false
    }

    fn reaches_element(&self, e: &Element, composition: Id) -> bool {
        e.kind.occurrence().is_some_and(|i| self.reaches(i.composition, composition))
    }

    /// What sheet `id` shows, bottom first, through its occurrences: every element but the
    /// occurrences themselves, each with what takes it to the sheet.
    pub fn placed(&self, sheet: Id) -> Vec<Placed<'_>> {
        let mut out = Vec::new();
        let Some(s) = self.sheet(sheet) else { return out };
        let projection = self.presentation_of(sheet).map_or_else(Projection::default, |p| p.projection);
        let start =
            Walk { path: Vec::new(), transform: DAffine2::IDENTITY, hidden: false, locked: false, paper: Some(s.paper), projection };
        self.walk(&s.elements, &start, &mut out);
        out
    }

    /// What the occurrences `path`, from sheet `sheet` down, open: the container the last one
    /// places and what takes its axes to the sheet's; none when the path does not lead there.
    pub fn opened(&self, sheet: Id, path: &[Id]) -> Option<(Container, DAffine2)> {
        let s = self.sheet(sheet)?;
        let (mut list, mut container, mut t, mut paper) =
            (s.elements.as_slice(), Container::Sheet(sheet), DAffine2::IDENTITY, Some(s.paper));
        for id in path {
            let i = list.iter().find(|e| e.id == *id)?.kind.occurrence()?;
            let b = self.composition(i.composition)?;
            t *= i.place.transform(paper);
            (list, container, paper) = (b.elements.as_slice(), Container::Composition(b.id), None);
        }
        Some((container, t))
    }

    fn walk<'a>(&'a self, list: &'a [Element], at: &Walk, out: &mut Vec<Placed<'a>>) {
        for e in list {
            let (hidden, locked) = (at.hidden || e.hidden, at.locked || e.locked);
            let ElementKind::Occurrence(i) = &e.kind else {
                out.push(Placed { element: e, path: at.path.clone(), transform: at.transform, hidden, locked, projection: at.projection });
                continue;
            };
            // Compositions never hold themselves (refused, and checked in a file).
            let Some(composition) = self.composition(i.composition).filter(|_| at.path.len() < MAX_NESTING) else { continue };
            let mut path = at.path.clone();
            path.push(e.id);
            let inner = Walk {
                path,
                transform: at.transform * i.place.transform(at.paper),
                hidden,
                locked,
                paper: None,
                projection: at.projection,
            };
            self.walk(&composition.elements, &inner, out);
        }
    }
}

/// How deep occurrences may nest in [`Drawing::placed`].
const MAX_NESTING: usize = 64;

/// Where [`Drawing::walk`] is.
struct Walk {
    path: Vec<Id>,
    transform: DAffine2,
    hidden: bool,
    locked: bool,
    /// The sheet's, for an occurrence right on it.
    paper: Option<Paper>,
    projection: Projection,
}

impl ElementKind {
    /// The view it hangs on: a projected view's base, a dimension's view.
    pub fn hung_on(&self) -> Option<Id> {
        match self {
            ElementKind::View(View { kind: ViewKind::Projected { base, .. }, .. }) => Some(*base),
            ElementKind::Dimension(d) => Some(d.view),
            _ => None,
        }
    }

    pub fn occurrence(&self) -> Option<&Occurrence> {
        match self {
            ElementKind::Occurrence(i) => Some(i),
            _ => None,
        }
    }

    /// Can it take a rotation? The others are drawn upright.
    fn turns(&self) -> bool {
        matches!(self, ElementKind::Line { .. } | ElementKind::Circle { .. } | ElementKind::Occurrence(_))
    }

    /// As it is once its composition's axes are `t`'s: what hangs on a view moves with it.
    fn placed_by(&self, t: DAffine2, rotation: f64) -> ElementKind {
        let mut out = self.clone();
        match &mut out {
            ElementKind::View(View { kind: ViewKind::Base { at, .. }, .. }) | ElementKind::Text { at, .. } => *at = t.transform_point2(*at),
            ElementKind::Circle { center, .. } => *center = t.transform_point2(*center),
            ElementKind::Line { from, to } => (*from, *to) = (t.transform_point2(*from), t.transform_point2(*to)),
            ElementKind::Rect { min, max } | ElementKind::Image { min, max, .. } => {
                (*min, *max) = (t.transform_point2(*min), t.transform_point2(*max));
            }
            ElementKind::Occurrence(i) => {
                i.place.at = t.transform_point2(i.place.at);
                i.place.rotation += rotation;
            }
            ElementKind::View(_) | ElementKind::Dimension(_) => {}
        }
        out
    }
}

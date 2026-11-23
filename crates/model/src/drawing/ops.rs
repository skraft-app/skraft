//! The drawing's operations (docs/ui/drawing.md, decision 0049): presentations, sheets, elements,
//! compositions and their instances.

use std::collections::HashMap;

use glam::{DAffine2, DAffine3, DVec2};
use serde::{Deserialize, Serialize};
use skraft_i18n::t;

use super::{
    Border, Composition, Container, Drawing, Element, ElementKind, Instance, Look, Paper, Place, Presentation, Projection, Sheet, Subject,
    View, ViewKind,
};
use crate::document::{name_base, numbered_name};
use crate::props::is_key;
use crate::{ComponentKind, Document, EntityPath, Error, Id, PlacedPart, Refusal, Result, Value};

/// What a field's value is set on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldOn {
    Presentation(Id),
    Sheet(Id),
    Instance(Id),
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

    pub(super) fn drawing_mut(&mut self) -> &mut Drawing {
        std::sync::Arc::make_mut(&mut self.drawing)
    }

    // ---- Presentations -------------------------------------------------------------------

    /// A presentation added last, « Présentation n » unless named.
    pub(crate) fn add_presentation(&mut self, name: Option<&str>) -> Result<Id> {
        let id = self.fresh_id();
        let n = self.drawing.presentations.len() + 1;
        let name = name.map(str::to_owned).unwrap_or_else(|| t!("drawing.presentation_name", n = n));
        self.drawing_mut().presentations.push(Presentation {
            id,
            name,
            projection: Projection::default(),
            props: Default::default(),
            sheets: Vec::new(),
        });
        Ok(id)
    }

    pub(crate) fn set_presentation(&mut self, presentation: Id, name: &str, projection: Projection) -> Result<()> {
        let p = self.presentation_mut(presentation)?;
        (p.name, p.projection) = (name.to_owned(), projection);
        Ok(())
    }

    fn presentation_mut(&mut self, id: Id) -> Result<&mut Presentation> {
        self.drawing_mut().presentations.iter_mut().find(|p| p.id == id).ok_or(Error::NotFound(id))
    }

    /// A presentation deleted, with its sheets.
    pub(crate) fn delete_presentation(&mut self, presentation: Id) -> Result<()> {
        let d = self.drawing_mut();
        let i = d.presentations.iter().position(|p| p.id == presentation).ok_or(Error::NotFound(presentation))?;
        d.presentations.remove(i);
        self.prune_compositions();
        self.prune_assets();
        Ok(())
    }

    pub(crate) fn move_presentation(&mut self, presentation: Id, index: usize) -> Result<()> {
        let d = self.drawing_mut();
        let i = d.presentations.iter().position(|p| p.id == presentation).ok_or(Error::NotFound(presentation))?;
        let p = d.presentations.remove(i);
        d.presentations.insert(index.min(d.presentations.len()), p);
        Ok(())
    }

    // ---- Sheets --------------------------------------------------------------------------

    /// Where a sheet goes: presentation `id`, else the first, made if there is none.
    fn presentation_for(&mut self, id: Option<Id>) -> Result<Id> {
        match id {
            Some(id) => self.drawing.presentation(id).map(|p| p.id).ok_or(Error::NotFound(id)),
            None => match self.drawing.presentations.first() {
                Some(p) => Ok(p.id),
                None => self.add_presentation(None),
            },
        }
    }

    pub(crate) fn add_sheet(&mut self, presentation: Option<Id>, name: Option<&str>, paper: Paper) -> Result<Id> {
        if !paper.is_valid() {
            return Err(Error::Invalid(Refusal::BadPaper));
        }
        let p = self.presentation_for(presentation)?;
        let id = self.fresh_id();
        let p = self.presentation_mut(p)?;
        let n = p.sheets.len() + 1;
        let name = name.map(str::to_owned).unwrap_or_else(|| t!("drawing.sheet_name", n = n));
        p.sheets.push(Sheet { id, name, paper, elements: Vec::new(), grid: 0.0, border: None, props: Default::default() });
        Ok(id)
    }

    fn sheet_mut(&mut self, id: Id) -> Result<&mut Sheet> {
        self.drawing_mut().presentations.iter_mut().flat_map(|p| &mut p.sheets).find(|s| s.id == id).ok_or(Error::NotFound(id))
    }

    pub(crate) fn set_sheet(&mut self, sheet: Id, name: &str, paper: Paper, grid: f64, border: Option<Border>) -> Result<()> {
        if !paper.is_valid() || !(0.0..=100.0).contains(&grid) || border.is_some_and(|b| !b.fits(paper)) {
            return Err(Error::Invalid(Refusal::BadPaper));
        }
        let s = self.sheet_mut(sheet)?;
        (s.name, s.paper, s.grid, s.border) = (name.to_owned(), paper, grid, border);
        Ok(())
    }

    pub(crate) fn delete_sheet(&mut self, sheet: Id) -> Result<()> {
        let d = self.drawing_mut();
        let p = d.presentations.iter_mut().find(|p| p.sheets.iter().any(|s| s.id == sheet)).ok_or(Error::NotFound(sheet))?;
        p.sheets.retain(|s| s.id != sheet);
        self.prune_compositions();
        self.prune_assets();
        Ok(())
    }

    /// A sheet moved to `index` of presentation `to` (none: its own).
    pub(crate) fn move_sheet(&mut self, sheet: Id, to: Option<Id>, index: usize) -> Result<()> {
        let from = self.drawing.presentation_of(sheet).ok_or(Error::NotFound(sheet))?.id;
        let to = to.unwrap_or(from);
        self.drawing.presentation(to).ok_or(Error::NotFound(to))?;
        let p = self.presentation_mut(from)?;
        let i = p.sheets.iter().position(|s| s.id == sheet).ok_or(Error::NotFound(sheet))?;
        let s = p.sheets.remove(i);
        let p = self.presentation_mut(to)?;
        p.sheets.insert(index.min(p.sheets.len()), s);
        Ok(())
    }

    /// A copy of sheet `sheet` at `index` of presentation `to`: its elements under fresh ids, what
    /// hangs on a view following it, its instances placing the same compositions; its name, or if
    /// that presentation has it the next free « Feuille n » for a default name, else numbered.
    /// Gives it.
    pub(crate) fn copy_sheet(&mut self, sheet: Id, to: Id, index: usize) -> Result<Id> {
        let source = self.drawing.sheet(sheet).ok_or(Error::NotFound(sheet))?.clone();
        self.drawing.presentation(to).ok_or(Error::NotFound(to))?;
        let elements = self.fresh_copies(&source.elements);
        let id = self.fresh_id();
        let p = self.presentation_mut(to)?;
        let name = {
            let taken = |n: &str| p.sheets.iter().any(|s| s.name == n);
            let number = source.name.rsplit(|c: char| !c.is_ascii_digit()).next().and_then(|n| n.parse::<usize>().ok());
            let default = number.is_some_and(|n| t!("drawing.sheet_name", n = n) == source.name);
            if !taken(&source.name) {
                source.name.clone()
            } else if default {
                (p.sheets.len() + 1..).map(|n| t!("drawing.sheet_name", n = n)).find(|n| !taken(n)).expect("a free number")
            } else {
                numbered_name(name_base(&source.name), 2, taken)
            }
        };
        p.sheets.insert(index.min(p.sheets.len()), Sheet { id, name, elements, ..source });
        Ok(id)
    }

    // ---- Elements ------------------------------------------------------------------------

    /// `kind` is fit to be in `container`, as element `own` if it is one already.
    fn check_element(&self, kind: &ElementKind, container: Container, own: Option<Id>) -> Result<()> {
        let finite = |v: DVec2| v.is_finite();
        let beside = |id: Id| {
            if self.drawing.container_of(id) == Some(container) { Ok(()) } else { Err(Error::Invalid(Refusal::SplitView)) }
        };
        let ok = match kind {
            ElementKind::View(v) => match &v.kind {
                ViewKind::Base { subject, look, scale, at } => {
                    let look_ok = match look {
                        Look::Standard(_) => true,
                        Look::Camera { forward, up, perspective } => {
                            forward.is_finite()
                                && up.is_finite()
                                && forward.length() > 1e-9
                                && perspective.is_none_or(|p| {
                                    p.eye.is_finite() && p.fov_y > 0.0 && p.fov_y < 3.0 && p.size.is_finite() && p.size.min_element() > 0.0
                                })
                        }
                    };
                    if look.frame().1.is_some() && own.is_some_and(|o| self.drawing.has_projected_views(o)) {
                        return Err(Error::Invalid(Refusal::ProjectedFromPerspective));
                    }
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
                        Some(_) => {
                            beside(*base)?;
                            distance.is_finite()
                        }
                    }
                }
            },
            ElementKind::Text { at, size, .. } => finite(*at) && size.is_finite() && *size > 0.0,
            ElementKind::Line { from, to } => finite(*from) && finite(*to),
            ElementKind::Rect { min, max } => finite(*min) && finite(*max),
            ElementKind::Circle { center, radius } => finite(*center) && radius.is_finite() && *radius > 0.0,
            ElementKind::Image { asset, min, max } => self.assets.contains_key(asset) && finite(*min) && finite(*max),
            ElementKind::Dimension(dim) => {
                if !matches!(self.drawing.element(dim.view).map(|e| &e.kind), Some(ElementKind::View(_))) {
                    return Err(Error::Invalid(Refusal::BadElement));
                }
                beside(dim.view)?;
                let anchor_ok = |a: &crate::Anchor| a.at.is_finite();
                match &dim.kind {
                    crate::DimKind::Linear { a, b, offset, .. } => anchor_ok(a) && anchor_ok(b) && offset.is_finite(),
                    crate::DimKind::Radius { edge, at, .. } => anchor_ok(edge) && finite(*at),
                    crate::DimKind::Angle { a, b, at } => anchor_ok(a) && anchor_ok(b) && finite(*at),
                }
            }
            ElementKind::Instance(i) => {
                self.drawing.composition(i.composition).ok_or(Error::NotFound(i.composition))?;
                if let Container::Composition(b) = container {
                    if self.drawing.reaches(i.composition, b) {
                        return Err(Error::Invalid(Refusal::CompositionLoop));
                    }
                    if i.place.corner.is_some() {
                        return Err(Error::Invalid(Refusal::BadElement));
                    }
                }
                i.place.is_valid() && i.props.iter().all(|(k, v)| is_key(k) && v.is_valid())
            }
        };
        if ok { Ok(()) } else { Err(Error::Invalid(Refusal::BadElement)) }
    }

    pub(crate) fn add_element(&mut self, to: Container, kind: &ElementKind) -> Result<Id> {
        if self.drawing.elements_in(to).is_none() {
            return Err(Error::NotFound(match to {
                Container::Sheet(id) | Container::Composition(id) => id,
            }));
        }
        self.check_element(kind, to, None)?;
        let id = self.fresh_id();
        let list = self.drawing_mut().elements_in_mut(to).expect("checked above");
        list.push(Element::new(id, kind.clone()));
        Ok(id)
    }

    pub(crate) fn set_element(&mut self, id: Id, kind: &ElementKind) -> Result<()> {
        let old = self.drawing.element(id).ok_or(Error::NotFound(id))?;
        if std::mem::discriminant(&old.kind) != std::mem::discriminant(kind) {
            return Err(Error::Invalid(Refusal::BadElement));
        }
        let container = self.drawing.container_of(id).ok_or(Error::NotFound(id))?;
        self.check_element(kind, container, Some(id))?;
        self.drawing_mut().element_mut(id).expect("found above").kind = kind.clone();
        if matches!(kind, ElementKind::Instance(_)) {
            self.prune_compositions();
        }
        self.prune_assets();
        Ok(())
    }

    /// Deletes the elements `ids`, and what hangs on them.
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
        for list in self.drawing_mut().lists_mut() {
            list.retain(|e| !gone.contains(&e.id));
        }
        self.prune_compositions();
        self.prune_assets();
        Ok(())
    }

    /// Elements hidden or shown, locked or not (what is `None` left as it is).
    pub(crate) fn set_element_state(&mut self, ids: &[Id], hidden: Option<bool>, locked: Option<bool>) -> Result<()> {
        if ids.is_empty() {
            return Err(Error::Invalid(Refusal::BadElement));
        }
        for id in ids {
            let e = self.drawing_mut().element_mut(*id).ok_or(Error::NotFound(*id))?;
            e.hidden = hidden.unwrap_or(e.hidden);
            e.locked = locked.unwrap_or(e.locked);
        }
        Ok(())
    }

    /// An element moved to `index` in its container's order (bottom first).
    pub(crate) fn move_element(&mut self, id: Id, index: usize) -> Result<()> {
        let c = self.drawing.container_of(id).ok_or(Error::NotFound(id))?;
        let list = self.drawing_mut().elements_in_mut(c).ok_or(Error::NotFound(id))?;
        let i = list.iter().position(|e| e.id == id).ok_or(Error::NotFound(id))?;
        let e = list.remove(i);
        list.insert(index.min(list.len()), e);
        Ok(())
    }

    // ---- Compositions --------------------------------------------------------------------------

    /// Drops the compositions neither named nor placed, those they alone placed after them.
    pub(crate) fn prune_compositions(&mut self) {
        loop {
            let used: std::collections::BTreeSet<Id> =
                self.drawing.containers().flat_map(|(_, l)| l).filter_map(|e| e.kind.instance().map(|i| i.composition)).collect();
            if !self.drawing.compositions.values().any(|b| !b.named && !used.contains(&b.id)) {
                return;
            }
            self.drawing_mut().compositions.retain(|id, b| b.named || used.contains(id));
        }
    }

    /// `name` if no other composition has it, else numbered.
    pub(super) fn free_composition_name(&self, name: &str, own: Option<Id>) -> String {
        let taken = |n: &str| self.drawing.compositions.values().any(|b| Some(b.id) != own && b.name == n);
        if taken(name) { numbered_name(name_base(name), 2, taken) } else { name.to_owned() }
    }

    /// Copies of `elements` under fresh ids, what hangs on one following it.
    pub(super) fn fresh_copies(&mut self, elements: &[Element]) -> Vec<Element> {
        let map: HashMap<Id, Id> = elements.iter().map(|e| (e.id, self.fresh_id())).collect();
        elements
            .iter()
            .map(|e| {
                let mut c = e.clone();
                c.id = map[&e.id];
                match &mut c.kind {
                    ElementKind::View(View { kind: ViewKind::Projected { base, .. }, .. }) => *base = *map.get(base).unwrap_or(base),
                    ElementKind::Dimension(d) => d.view = *map.get(&d.view).unwrap_or(&d.view),
                    _ => {}
                }
                c
            })
            .collect()
    }

    /// The elements `ids` — one container's — gathered into a new composition placed where they
    /// are, with the views and dimensions tied to them. « Composition#n » unless named. Gives
    /// the instance.
    pub(crate) fn make_composition(&mut self, ids: &[Id], name: Option<&str>) -> Result<Id> {
        let first = *ids.first().ok_or(Error::Invalid(Refusal::BadElement))?;
        let container = self.drawing.container_of(first).ok_or(Error::NotFound(first))?;
        let members = self.families(container, ids)?;
        let paper = self.paper_of(container);
        let (composition, instance) = (self.fresh_id(), self.fresh_id());
        let composition_name = match name {
            Some(n) => self.free_composition_name(n, None),
            None => {
                let base = t!("drawing.composition.name");
                numbered_name(&base, 1, |n| self.drawing.compositions.values().any(|b| b.name == n))
            }
        };
        let d = self.drawing_mut();
        let list = d.elements_in_mut(container).ok_or(Error::NotFound(first))?;
        // Where the topmost was: what is drawn over them stays over them.
        let last = list.iter().rposition(|e| members.contains(&e.id)).ok_or(Error::NotFound(first))?;
        let at = last + 1 - members.len();
        let (mut taken, kept): (Vec<Element>, Vec<Element>) = std::mem::take(list).into_iter().partition(|e| members.contains(&e.id));
        *list = kept;
        list.insert(
            at,
            Element::new(
                instance,
                ElementKind::Instance(Instance { composition, place: Place::free(DVec2::ZERO), props: Default::default() }),
            ),
        );
        // A composition has no paper: what was anchored on the sheet stays where it is.
        for e in &mut taken {
            if let ElementKind::Instance(i) = &mut e.kind {
                i.place = Place { corner: None, at: i.place.origin(paper), rotation: i.place.rotation };
            }
        }
        d.compositions.insert(
            composition,
            Composition { id: composition, name: composition_name, named: name.is_some(), elements: taken, source: None },
        );
        Ok(instance)
    }

    fn paper_of(&self, container: Container) -> Option<Paper> {
        match container {
            Container::Sheet(s) => self.drawing.sheet(s).map(|s| s.paper),
            Container::Composition(_) => None,
        }
    }

    /// Instance `id` and its composition, with its container.
    fn instance(&self, id: Id) -> Result<(Container, &Element, &Instance, &Composition)> {
        let container = self.drawing.container_of(id).ok_or(Error::NotFound(id))?;
        let e = self.drawing.element(id).ok_or(Error::NotFound(id))?;
        let i = e.kind.instance().ok_or(Error::Invalid(Refusal::BadElement))?;
        let b = self.drawing.composition(i.composition).ok_or(Error::NotFound(i.composition))?;
        Ok((container, e, i, b))
    }

    /// An instance replaced by copies of what its composition holds, where it shows them. Refused
    /// when turned over what has no rotation of its own. Gives the copies.
    pub(crate) fn explode_instance(&mut self, id: Id) -> Result<Vec<Id>> {
        let (container, e, i, b) = self.instance(id)?;
        let (hidden, locked, rotation) = (e.hidden, e.locked, i.place.rotation);
        if rotation != 0.0 && b.elements.iter().any(|x| !x.kind.turns()) {
            return Err(Error::Invalid(Refusal::TurnedInstance));
        }
        let t: DAffine2 = i.place.transform(self.paper_of(container));
        let elements = b.elements.clone();
        let mut copies = self.fresh_copies(&elements);
        for c in &mut copies {
            c.kind = c.kind.placed_by(t, rotation);
            c.hidden |= hidden;
            c.locked |= locked;
        }
        let ids: Vec<Id> = copies.iter().map(|c| c.id).collect();
        let list = self.drawing_mut().elements_in_mut(container).ok_or(Error::NotFound(id))?;
        let at = list.iter().position(|x| x.id == id).ok_or(Error::NotFound(id))?;
        list.splice(at..=at, copies);
        self.prune_compositions();
        self.prune_assets();
        Ok(ids)
    }

    /// Instance `id` given a composition of its own, a copy of the one it shares (nothing to do when
    /// it is alone). Gives its composition.
    pub(crate) fn make_instance_unique(&mut self, id: Id) -> Result<Id> {
        let (_, _, i, b) = self.instance(id)?;
        let composition = i.composition;
        if self.drawing.instances_of(composition).len() < 2 {
            return Ok(composition);
        }
        let (elements, named, name) = (b.elements.clone(), b.named, b.name.clone());
        let copies = self.fresh_copies(&elements);
        let new = self.fresh_id();
        let name = self.free_composition_name(&name, None);
        let d = self.drawing_mut();
        d.compositions.insert(new, Composition { id: new, name, named, elements: copies, source: None });
        if let Some(ElementKind::Instance(i)) = d.element_mut(id).map(|e| &mut e.kind) {
            i.composition = new;
        }
        Ok(new)
    }

    /// A composition named: it now stays while placed nowhere.
    pub(crate) fn rename_composition(&mut self, composition: Id, name: &str) -> Result<()> {
        self.drawing.composition(composition).ok_or(Error::NotFound(composition))?;
        if name.trim().is_empty() {
            return Err(Error::Invalid(Refusal::BadElement));
        }
        let name = self.free_composition_name(name, Some(composition));
        let b = self.drawing_mut().compositions.get_mut(&composition).expect("found above");
        (b.name, b.named) = (name, true);
        Ok(())
    }

    /// A composition deleted, with its instances.
    pub(crate) fn delete_composition(&mut self, composition: Id) -> Result<()> {
        self.drawing.composition(composition).ok_or(Error::NotFound(composition))?;
        let instances = self.drawing.instances_of(composition);
        if !instances.is_empty() {
            self.delete_elements(&instances)?;
        }
        self.drawing_mut().compositions.remove(&composition);
        self.prune_compositions();
        self.prune_assets();
        Ok(())
    }

    /// The compositions shown on no sheet deleted, with what they place of each other.
    pub(crate) fn purge_compositions(&mut self) {
        let on_sheets = self.drawing.counts_on_sheets();
        self.drawing_mut().compositions.retain(|id, _| on_sheets.contains_key(id));
        self.prune_assets();
    }

    // ---- Fields --------------------------------------------------------------------------

    /// A field's value set on `on`, or cleared.
    pub(crate) fn set_field(&mut self, on: FieldOn, key: &str, value: Option<&Value>) -> Result<()> {
        if !is_key(key) {
            return Err(Error::Invalid(Refusal::BadPropKey));
        }
        if value.is_some_and(|v| !v.is_valid()) {
            return Err(Error::Invalid(Refusal::BadPropValue));
        }
        let value = value.cloned();
        match on {
            FieldOn::Presentation(id) => self.presentation_mut(id)?.props.set(key, value),
            FieldOn::Sheet(id) => self.sheet_mut(id)?.props.set(key, value),
            FieldOn::Instance(id) => {
                let e = self.drawing_mut().element_mut(id).ok_or(Error::NotFound(id))?;
                match &mut e.kind {
                    ElementKind::Instance(i) => i.props.set(key, value),
                    _ => return Err(Error::Invalid(Refusal::BadElement)),
                }
            }
        }
        Ok(())
    }

    /// The value of field `key` for an element of sheet `sheet` under the instances `path`
    /// (outermost first): the innermost instance's, the sheet's, its presentation's, else the
    /// document's. A key without a namespace is the user's (`client`: `user.client`).
    pub fn field(&self, sheet: Id, path: &[Id], key: &str) -> Option<&Value> {
        let key = &super::field_key(key);
        let d = &self.drawing;
        path.iter()
            .rev()
            .find_map(|id| d.element(*id)?.kind.instance()?.props.get(key))
            .or_else(|| d.sheet(sheet)?.props.get(key))
            .or_else(|| d.presentation_of(sheet)?.props.get(key))
            .or_else(|| self.components.get(&self.root)?.props.get(key))
    }
}

//! Copying a drawing's elements (docs/ui/drawing.md#copy-and-paste) and moving them between
//! sheets: what hangs on a view goes with it, an instance stays an instance of its composition.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use glam::{DAffine2, DVec2};

use super::{Composition, Container, Drawing, Element, ElementKind, Paper, Presentation, Projection, Sheet, Subject, View, ViewKind};
use crate::{Document, Error, Id, Refusal, Result};

impl Document {
    /// The elements `ids` of one container with their families: each up to its first base, then
    /// all that hangs on that one. In the container's order.
    pub fn families(&self, container: Container, ids: &[Id]) -> Result<Vec<Id>> {
        let list = self.drawing.elements_in(container).ok_or(Error::Invalid(Refusal::BadElement))?;
        let mut members = Vec::new();
        for id in ids {
            if !list.iter().any(|e| e.id == *id) {
                return Err(Error::Invalid(Refusal::BadElement));
            }
            let mut root = *id;
            while let Some(b) = self.drawing.element(root).and_then(|e| e.kind.hung_on()) {
                root = b;
            }
            for m in std::iter::once(root).chain(self.drawing.projections_of(root)) {
                if !members.contains(&m) {
                    members.push(m);
                }
            }
        }
        Ok(list.iter().map(|e| e.id).filter(|id| members.contains(id)).collect())
    }

    /// The elements `ids` of `container` as a fragment: a document holding a drawing of one
    /// sheet (of the paper they were on) with them and their families, the compositions they
    /// place at any depth, their images. Ids kept.
    pub fn elements_fragment(&self, container: Container, ids: &[Id]) -> Result<Document> {
        let members = self.families(container, ids)?;
        if members.is_empty() {
            return Err(Error::Invalid(Refusal::BadElement));
        }
        let list = self.drawing.elements_in(container).ok_or(Error::Invalid(Refusal::BadElement))?;
        let elements: Vec<Element> = list.iter().filter(|e| members.contains(&e.id)).cloned().collect();
        let mut compositions: BTreeMap<Id, Composition> = BTreeMap::new();
        let mut todo: Vec<Id> = elements.iter().filter_map(|e| e.kind.instance().map(|i| i.composition)).collect();
        while let Some(id) = todo.pop() {
            if compositions.contains_key(&id) {
                continue;
            }
            let c = self.drawing.composition(id).ok_or(Error::NotFound(id))?;
            todo.extend(c.elements.iter().filter_map(|e| e.kind.instance().map(|i| i.composition)));
            compositions.insert(id, c.clone());
        }
        let paper = match container {
            Container::Sheet(s) => self.drawing.sheet(s).map_or(Paper::A3, |s| s.paper),
            Container::Composition(_) => Paper::A3,
        };
        let mut out = Document::new();
        out.next_id = self.next_id;
        let (sheet, presentation) = (out.fresh_id(), out.fresh_id());
        let sheet = Sheet { id: sheet, name: String::new(), paper, elements, grid: 0.0, border: None, props: Default::default() };
        let presentation = Presentation {
            id: presentation,
            name: String::new(),
            projection: Projection::default(),
            props: Default::default(),
            sheets: vec![sheet],
        };
        out.drawing = Arc::new(Drawing { presentations: vec![presentation], compositions });
        let images: Vec<String> = out.drawing.images().map(str::to_owned).collect();
        out.assets = self.assets.iter().filter(|(id, _)| images.contains(id)).map(|(k, v)| (k.clone(), v.clone())).collect();
        Ok(out)
    }

    /// Do the components the views of `fragment` show still stand here?
    fn subjects_stand(&self, fragment: &Document) -> bool {
        fragment.drawing.containers().flat_map(|(_, list)| list).all(|e| match &e.kind {
            ElementKind::View(View { kind: ViewKind::Base { subject: Subject::Component(c), .. }, .. }) => self.components.contains_key(c),
            _ => true,
        })
    }

    /// Composition `id` of `fragment`, copied from this document, back if it went since (its
    /// last instance cut): under its id, which the state copied from keeps from being reused,
    /// so that every paste places the same one.
    fn restore_composition(&mut self, fragment: &Document, id: Id) -> Result<()> {
        if self.drawing.composition(id).is_some() {
            return Ok(());
        }
        let c = fragment.drawing.composition(id).ok_or(Error::NotFound(id))?;
        for e in &c.elements {
            if let Some(i) = e.kind.instance() {
                self.restore_composition(fragment, i.composition)?;
            }
        }
        let free = c.elements.iter().all(|e| self.drawing.element(e.id).is_none());
        let elements = if free { c.elements.clone() } else { self.fresh_copies(&c.elements) };
        let name = self.free_composition_name(&c.name, None);
        self.drawing_mut().compositions.insert(id, Composition { elements, name, ..c.clone() });
        Ok(())
    }

    /// `fragment` pasted on top of `to`, moved by `offset`. `linked`: copied from this document
    /// in a state it still knows, so its instances stay those of the same compositions and its
    /// views keep showing the model. Else as a template's: no view nor dimension, the
    /// compositions brought once. Gives what was added.
    pub(crate) fn paste_elements(&mut self, fragment: &Document, to: Container, offset: DVec2, linked: bool) -> Result<Vec<Id>> {
        if self.drawing.elements_in(to).is_none() {
            return Err(Error::NotFound(match to {
                Container::Sheet(id) | Container::Composition(id) => id,
            }));
        }
        let from = fragment.drawing.presentations.first().ok_or(Error::Invalid(Refusal::BadElement))?;
        let source = from.sheets.first().ok_or(Error::Invalid(Refusal::BadElement))?;
        let paper = source.paper;
        let mut copies = if linked && self.subjects_stand(fragment) {
            for (id, asset) in &fragment.assets {
                self.assets.insert(id.clone(), asset.clone());
            }
            for e in &source.elements {
                if let Some(i) = e.kind.instance() {
                    self.restore_composition(fragment, i.composition)?;
                }
            }
            self.fresh_copies(&source.elements)
        } else {
            let plain = fragment.template_of(from.id)?;
            let kept = plain.drawing.presentations[0].sheets[0].elements.clone();
            self.brought_elements(&plain, &kept, &mut HashMap::new())?
        };
        if copies.is_empty() {
            return Err(Error::Invalid(Refusal::NothingToPaste));
        }
        let t = DAffine2::from_translation(offset);
        for c in &mut copies {
            if let ElementKind::Instance(i) = &mut c.kind
                && let Container::Composition(b) = to
            {
                if self.drawing.reaches(i.composition, b) {
                    return Err(Error::Invalid(Refusal::CompositionLoop));
                }
                // A composition has no paper: where it was on the sheet.
                i.place = i.place.with_corner(None, paper);
            }
            c.kind = c.kind.placed_by(t, 0.0);
        }
        let ids: Vec<Id> = copies.iter().map(|c| c.id).collect();
        self.drawing_mut().elements_in_mut(to).ok_or(Error::Invalid(Refusal::BadElement))?.extend(copies);
        Ok(ids)
    }

    /// The elements `ids` of a sheet, with their families, moved (or copied, `copy`) on top of
    /// sheet `to`, by `offset` — an instance anchored to the paper keeps its corner and offset.
    /// Gives them as they are there.
    pub(crate) fn move_elements(&mut self, ids: &[Id], to: Id, offset: DVec2, copy: bool) -> Result<Vec<Id>> {
        let first = *ids.first().ok_or(Error::Invalid(Refusal::BadElement))?;
        let from = self.drawing.container_of(first).ok_or(Error::NotFound(first))?;
        self.drawing.sheet(to).ok_or(Error::NotFound(to))?;
        if !matches!(from, Container::Sheet(_)) {
            return Err(Error::Invalid(Refusal::BadElement));
        }
        let target = Container::Sheet(to);
        let members = self.families(from, ids)?;
        if from == target && !copy {
            return Ok(members);
        }
        let list = self.drawing.elements_in(from).ok_or(Error::NotFound(first))?;
        let taken: Vec<Element> = list.iter().filter(|e| members.contains(&e.id)).cloned().collect();
        let mut moved = if copy { self.fresh_copies(&taken) } else { taken };
        let t = DAffine2::from_translation(offset);
        for e in &mut moved {
            if !e.kind.instance().is_some_and(|i| i.place.corner.is_some()) {
                e.kind = e.kind.placed_by(t, 0.0);
            }
        }
        let out: Vec<Id> = moved.iter().map(|e| e.id).collect();
        let d = self.drawing_mut();
        if !copy {
            d.elements_in_mut(from).ok_or(Error::NotFound(first))?.retain(|e| !members.contains(&e.id));
        }
        d.elements_in_mut(target).ok_or(Error::NotFound(to))?.extend(moved);
        Ok(out)
    }
}

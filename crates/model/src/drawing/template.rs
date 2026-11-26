//! Templates (docs/ui/drawing.md#templates, decision 0049): a presentation, its sheets and their
//! definitions, without views, in a file of its own.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use glam::DVec2;
use skraft_i18n::t;

use super::{Border, Composition, Corner, Drawing, Element, ElementKind, Occurrence, Paper, Place, Presentation, Sheet};
use crate::{Document, Error, Id, Result};

/// The composition of the template Skraft comes with, whatever the language it was made in.
const DEFAULT_TITLE_BLOCK: &str = "skraft:title-block";

/// What a template keeps of `elements`: no view shows a model there, nor a dimension.
fn without_views(elements: &[Element]) -> Vec<Element> {
    elements.iter().filter(|e| !matches!(e.kind, ElementKind::View(_) | ElementKind::Dimension(_))).cloned().collect()
}

/// The name a composition goes by as a template's: the hash of what a template keeps of it. The same
/// composition saved twice has the same one, and a document holding it knows it when it comes back.
fn source_of(b: &Composition) -> Option<String> {
    let kept = Composition { elements: without_views(&b.elements), source: None, ..b.clone() };
    serde_json::to_vec(&kept).ok().map(|json| crate::assets::id_of(&json))
}

impl Document {
    /// Presentation `id` as a template: a document holding only a drawing — its sheets, the
    /// compositions they place, all without views (a composition left empty by that goes, with its
    /// occurrences), the images they show. Each composition is given the name of its source, by which
    /// it is brought into a document once.
    pub fn template_of(&self, presentation: Id) -> Result<Document> {
        let p = self.drawing.presentation(presentation).ok_or(Error::NotFound(presentation))?;
        let mut sheets: Vec<Sheet> = p.sheets.iter().map(|s| Sheet { elements: without_views(&s.elements), ..s.clone() }).collect();
        let mut compositions: BTreeMap<Id, Composition> = BTreeMap::new();
        let mut todo: Vec<Id> =
            sheets.iter().flat_map(|s| &s.elements).filter_map(|e| e.kind.occurrence().map(|i| i.composition)).collect();
        while let Some(id) = todo.pop() {
            if compositions.contains_key(&id) {
                continue;
            }
            let b = self.drawing.composition(id).ok_or(Error::NotFound(id))?;
            let mut kept = Composition { elements: without_views(&b.elements), ..b.clone() };
            todo.extend(kept.elements.iter().filter_map(|e| e.kind.occurrence().map(|i| i.composition)));
            if kept.source.is_none() {
                kept.source = source_of(b);
            }
            compositions.insert(id, kept);
        }
        // Compositions that held only views, and those that placed only them.
        loop {
            let empty: Vec<Id> = compositions.values().filter(|b| b.elements.is_empty()).map(|b| b.id).collect();
            if empty.is_empty() {
                break;
            }
            compositions.retain(|id, _| !empty.contains(id));
            let keep = |e: &Element| !e.kind.occurrence().is_some_and(|i| empty.contains(&i.composition));
            compositions.values_mut().for_each(|b| b.elements.retain(keep));
            sheets.iter_mut().for_each(|s| s.elements.retain(keep));
        }
        let mut t = Document::new();
        t.next_id = self.next_id;
        t.drawing = Arc::new(Drawing { presentations: vec![Presentation { sheets, ..p.clone() }], compositions });
        let images: Vec<String> = t.drawing.images().map(str::to_owned).collect();
        t.assets = self.assets.iter().filter(|(id, _)| images.contains(id)).map(|(k, v)| (k.clone(), v.clone())).collect();
        Ok(t)
    }

    /// The template Skraft comes with: A3 landscape, a border 10 mm in, and at its bottom right
    /// corner the composition « Cartouche » — the document's name, the sheet's, the date, the scale,
    /// the sheet's number.
    pub fn default_template() -> Document {
        let mut t = Document::new();
        let v = DVec2::new;
        let text = |at: DVec2, text: String, size: f64| ElementKind::Text { at, text, size };
        // In the composition's axes: its bottom right corner at the origin.
        let kinds = vec![
            ElementKind::Rect { min: v(-160.0, 0.0), max: v(0.0, 36.0) },
            ElementKind::Line { from: v(-160.0, 24.0), to: v(0.0, 24.0) },
            ElementKind::Line { from: v(-160.0, 12.0), to: v(0.0, 12.0) },
            ElementKind::Line { from: v(-70.0, 0.0), to: v(-70.0, 24.0) },
            text(v(-157.0, 27.5), "{document}".into(), 5.0),
            text(v(-157.0, 15.5), t!("drawing.template.sheet"), 2.5),
            text(v(-157.0, 3.5), t!("drawing.template.date"), 2.5),
            text(v(-67.0, 15.5), t!("drawing.template.scale"), 2.5),
            text(v(-67.0, 3.5), t!("drawing.template.number"), 2.5),
        ];
        let elements: Vec<Element> = kinds.into_iter().map(|kind| Element::new(t.fresh_id(), kind)).collect();
        let (composition, occurrence, sheet, presentation) = (t.fresh_id(), t.fresh_id(), t.fresh_id(), t.fresh_id());
        let d = t.drawing_mut();
        d.compositions.insert(
            composition,
            Composition {
                id: composition,
                name: t!("drawing.composition.title_block"),
                named: true,
                elements,
                source: Some(DEFAULT_TITLE_BLOCK.to_owned()),
            },
        );
        let place = Place::anchored(Corner::BottomRight, v(-10.0, 10.0));
        d.presentations.push(Presentation {
            id: presentation,
            name: t!("drawing.template.default"),
            projection: Default::default(),
            props: Default::default(),
            sheets: vec![Sheet {
                id: sheet,
                name: t!("drawing.sheet_name", n = 1),
                paper: Paper::A3,
                elements: vec![Element::new(
                    occurrence,
                    ElementKind::Occurrence(Occurrence { composition, place, props: Default::default() }),
                )],
                grid: 0.0,
                border: Some(Border { margin: 10.0, width: 0.25 }),
                props: Default::default(),
            }],
        });
        t
    }

    /// Composition `id` of `template` in this document: the one already brought from it if there
    /// is one, else a copy with what it places, its images.
    fn bring_composition(&mut self, template: &Document, id: Id, brought: &mut HashMap<Id, Id>) -> Result<Id> {
        if let Some(b) = brought.get(&id) {
            return Ok(*b);
        }
        let b = template.drawing.composition(id).ok_or(Error::NotFound(id))?;
        // Brought before, or the very composition the template was saved from.
        let known = b.source.as_ref().and_then(|src| {
            self.drawing
                .compositions
                .values()
                .find(|x| x.source.as_ref() == Some(src) || (x.source.is_none() && source_of(x).as_ref() == Some(src)))
        });
        if let Some(k) = known {
            let k = k.id;
            brought.insert(id, k);
            return Ok(k);
        }
        let elements = self.brought_elements(template, &b.elements, brought)?;
        let new = self.fresh_id();
        let name = self.free_composition_name(&b.name, None);
        self.drawing_mut().compositions.insert(new, Composition { id: new, name, named: b.named, elements, source: b.source.clone() });
        brought.insert(id, new);
        Ok(new)
    }

    /// Copies of a template's `elements` here: fresh ids, the compositions they place brought in,
    /// their images taken.
    pub(super) fn brought_elements(
        &mut self,
        template: &Document,
        elements: &[Element],
        brought: &mut HashMap<Id, Id>,
    ) -> Result<Vec<Element>> {
        let mut copies = self.fresh_copies(elements);
        for c in &mut copies {
            match &mut c.kind {
                ElementKind::Occurrence(i) => i.composition = self.bring_composition(template, i.composition, brought)?,
                ElementKind::Image { asset, .. } => {
                    if let Some(a) = template.assets.get(asset) {
                        self.assets.insert(asset.clone(), a.clone());
                    }
                }
                _ => {}
            }
        }
        Ok(copies)
    }

    /// A sheet made from `from`, a sheet of `template`, last in `presentation`.
    fn sheet_from(
        &mut self,
        template: &Document,
        from: &Sheet,
        presentation: Option<Id>,
        name: Option<&str>,
        brought: &mut HashMap<Id, Id>,
    ) -> Result<Id> {
        let elements = self.brought_elements(template, &from.elements, brought)?;
        let id = self.add_sheet(presentation, name, from.paper)?;
        let s = self.drawing_mut().presentations.iter_mut().flat_map(|p| &mut p.sheets).find(|s| s.id == id).ok_or(Error::NotFound(id))?;
        (s.elements, s.grid, s.border, s.props) = (elements, from.grid, from.border, from.props.clone());
        Ok(id)
    }

    /// A sheet added from `template`'s sheet `sheet` (none: its first), to `presentation` (none:
    /// the first), « Feuille n » unless named: the template's own sheet name is the template's.
    pub(crate) fn add_sheet_from(
        &mut self,
        template: &Document,
        sheet: Option<Id>,
        presentation: Option<Id>,
        name: Option<&str>,
    ) -> Result<Id> {
        let from = match sheet {
            Some(s) => Some(template.drawing.sheet(s).ok_or(Error::NotFound(s))?),
            None => template.drawing.sheets().next(),
        };
        match from {
            Some(from) => self.sheet_from(template, from, presentation, name, &mut HashMap::new()),
            None => self.add_sheet(presentation, name, Paper::A3),
        }
    }

    /// A presentation made from `template`'s, under its name (numbered if one has it), its
    /// sheets keeping theirs.
    pub(crate) fn add_presentation_from(&mut self, template: &Document, name: Option<&str>) -> Result<Id> {
        let from = template.drawing.presentations.first();
        let name = name.or(from.map(|p| p.name.as_str())).map(|n| {
            let taken = |x: &str| self.drawing.presentations.iter().any(|p| p.name == x);
            if taken(n) { crate::document::numbered_name(crate::document::name_base(n), 2, taken) } else { n.to_owned() }
        });
        let id = self.add_presentation(name.as_deref())?;
        if let Some(from) = from {
            let mut brought = HashMap::new();
            for s in &from.sheets {
                self.sheet_from(template, s, Some(id), Some(&s.name), &mut brought)?;
            }
            let p = self.drawing_mut().presentations.iter_mut().find(|p| p.id == id).ok_or(Error::NotFound(id))?;
            (p.projection, p.props) = (from.projection, from.props.clone());
        }
        Ok(id)
    }
}

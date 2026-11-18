//! The file format: versioned JSON of the document (docs/io/file-format.md).
//!
//! Version 1: the app is not published yet, the format changes without a new version and
//! without reading older files.

use serde::{Deserialize, Serialize};

use crate::document::{ComponentKind, Document, Id};
use crate::{Error, FileError, Result};

/// The `format` field of every Skraft file.
pub const FORMAT: &str = "skraft";
/// The version this build writes, and the newest it reads.
pub const VERSION: u32 = 1;

#[derive(Serialize)]
struct FileOut<'a> {
    format: &'a str,
    version: u32,
    document: &'a Document,
}

#[derive(Deserialize)]
struct Header {
    format: String,
    version: u32,
}

#[derive(Deserialize)]
struct FileIn<D> {
    document: D,
}

fn file_err(e: impl std::fmt::Display) -> Error {
    Error::File(FileError::Unreadable(e.to_string()))
}

impl Document {
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(&FileOut { format: FORMAT, version: VERSION, document: self }).map_err(file_err)
    }

    pub fn from_json(text: &str) -> Result<Document> {
        let h: Header = serde_json::from_str(text).map_err(file_err)?;
        if h.format != FORMAT {
            return Err(Error::File(FileError::NotSkraft(h.format)));
        }
        if h.version > VERSION {
            return Err(Error::File(FileError::TooNew(h.version)));
        }
        let mut doc = serde_json::from_str::<FileIn<Document>>(text).map_err(file_err)?.document;
        doc.check()?;
        doc.settle_names(None);
        Ok(doc)
    }

    /// The structural invariants a file must keep: the root an assembly, known components, no
    /// cycle, ids below `next_id` — unique in the document for components, within their
    /// assembly for occurrences and within their part for bodies and their definitions (docs/model/data-model.md),
    /// every body's definition there, guides within their
    /// component and apart from its occurrences or bodies —, a positive body in every part, its
    /// shape there exactly when it is not plain ([`Part::is_plain`]), and only its bodies
    /// marked failed.
    fn check(&self) -> Result<()> {
        self.assembly(self.root).map_err(|_| Error::File(FileError::RootNotAssembly))?;
        let mut seen = std::collections::HashSet::new();
        let mut fresh = |id: Id, bad: fn(Id) -> FileError| {
            if id.0 >= self.next_id || !seen.insert(id) {
                return Err(Error::File(bad(id)));
            }
            Ok(())
        };
        let below = |id: Id| if id.0 < self.next_id { Ok(()) } else { Err(Error::File(FileError::BadOccurrenceId(id))) };
        for id in self.materials.keys() {
            if id.0 >= self.next_id {
                return Err(Error::File(FileError::BadMaterialId(*id)));
            }
        }
        let material = |m: &Option<Id>| match m {
            Some(id) if !self.materials.contains_key(id) => Err(Error::File(FileError::UnknownMaterial(*id))),
            _ => Ok(()),
        };
        for (key, c) in &self.components {
            if *key != c.id {
                return Err(Error::File(FileError::Misfiled { id: c.id, key: *key }));
            }
            fresh(c.id, FileError::BadComponentId)?;
            if !c.behavior.is_valid() {
                return Err(Error::File(FileError::BadBehavior(c.id)));
            }
            // A component's guides, apart from its occurrences or bodies.
            let mut here = std::collections::HashSet::new();
            for g in &c.guides {
                if g.id.0 >= self.next_id || !g.shape.is_valid() || !here.insert(g.id) {
                    return Err(Error::File(FileError::BadGuide(g.id)));
                }
            }
            match &c.kind {
                ComponentKind::Part(p) => {
                    for d in &p.defs {
                        if d.id.0 >= self.next_id || !here.insert(d.id) {
                            return Err(Error::File(FileError::BadBodyId(d.id)));
                        }
                    }
                    for b in p.all_bodies() {
                        if b.id.0 >= self.next_id || !here.insert(b.id) || p.def(b.def).is_none() {
                            return Err(Error::File(FileError::BadBodyId(b.id)));
                        }
                    }
                    // A group: its failures among its children, no group holding itself, at any
                    // depth. Without a positive child it is empty, as a part may be (docs/model/bodies.md
                    // « Empty parts »).
                    for d in &p.defs {
                        let Some(g) = &d.group else { continue };
                        if g.failed.iter().any(|id| !g.children.iter().any(|b| b.id == *id))
                            || crate::document::used_defs(&p.defs, &g.children).contains(&d.id)
                        {
                            return Err(Error::File(FileError::BadBodyId(d.id)));
                        }
                    }
                    if p.failed.iter().any(|id| p.body(*id).is_none()) {
                        return Err(Error::File(FileError::ShapeOutOfStep(c.id)));
                    }
                    for m in p.faces.values() {
                        material(&Some(*m))?;
                    }
                    if p.is_plain() != p.shape.is_none() {
                        return Err(Error::File(FileError::ShapeOutOfStep(c.id)));
                    }
                }
                ComponentKind::Assembly(a) => {
                    for o in a.occurrences() {
                        below(o.id)?;
                        material(&o.material)?;
                        if !here.insert(o.id) {
                            return Err(Error::File(FileError::DuplicateOccurrence { occurrence: o.id, assembly: c.id }));
                        }
                        self.component(o.component)?;
                        if o.component == self.root || self.contains_component(o.component, c.id) {
                            return Err(Error::Cycle);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

//! The file format: versioned JSON of the document (docs/ARCHITECTURE.md §8).
//!
//! Version 2 is the parts / assemblies model (§2). Version 1 files (free solids, groups and
//! components) are converted when opened ([`v1`]).

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::document::{ComponentKind, Document, Id};
use crate::{Error, FileError, Result};

/// The `format` field of every SolidSketch file.
pub const FORMAT: &str = "solidsketch";
/// The version this build writes, and the newest it reads.
pub const VERSION: u32 = 2;

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
            return Err(Error::File(FileError::NotSolidSketch(h.format)));
        }
        if h.version > VERSION {
            return Err(Error::File(FileError::TooNew(h.version)));
        }
        let doc = match h.version {
            1 => serde_json::from_str::<FileIn<v1::Document>>(text).map_err(file_err)?.document.convert()?,
            _ => serde_json::from_str::<FileIn<Document>>(text).map_err(file_err)?.document,
        };
        doc.check()?;
        Ok(doc)
    }

    /// The structural invariants a file must keep: the root an assembly, known components, no
    /// cycle, ids below `next_id` — unique in the document for components and machinings,
    /// within their assembly for occurrences (§2.6) —, a part's shape there exactly when it has
    /// machinings.
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
            match &c.kind {
                ComponentKind::Part(p) => {
                    for m in &p.machinings {
                        fresh(m.id, FileError::BadMachiningId)?;
                    }
                    for m in p.faces.values() {
                        material(&Some(*m))?;
                    }
                    if p.machinings.is_empty() != p.shape.is_none() {
                        return Err(Error::File(FileError::ShapeOutOfStep(c.id)));
                    }
                }
                ComponentKind::Assembly(a) => {
                    let mut here = std::collections::HashSet::new();
                    for o in &a.occurrences {
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

/// Version 1: definitions (root, groups, components) holding free solids — in their
/// definition's coordinates — and instances.
mod v1 {
    use glam::DAffine3;
    use solidsketch_kernel::Solid;

    use super::*;
    use crate::document::{Assembly, Component, Occurrence, Part};
    use crate::settings::Settings;

    #[derive(Deserialize)]
    pub struct Document {
        definitions: BTreeMap<Id, Definition>,
        root: Id,
        next_id: u64,
        #[serde(default)]
        settings: Settings,
    }

    #[derive(Deserialize, PartialEq)]
    enum Kind {
        Root,
        Group,
        Component,
    }

    #[derive(Deserialize)]
    struct Definition {
        id: Id,
        name: String,
        kind: Kind,
        entities: Vec<Entity>,
    }

    #[derive(Deserialize)]
    enum Entity {
        Solid { id: Id, name: String, solid: Arc<Solid> },
        Instance { id: Id, name: String, definition: Id, transform: DAffine3 },
    }

    impl Document {
        /// Definitions become assemblies (a component's name is the user's: kept while unused;
        /// a group's is not), instances their occurrences. A free solid becomes a part — its
        /// axes those of its definition, so nothing moves and no kernel is needed — and the
        /// part's one occurrence, under the solid's id so selections and names carry over.
        pub fn convert(self) -> Result<super::Document> {
            if self.definitions.get(&self.root).is_none_or(|d| d.kind != Kind::Root) {
                return Err(Error::File(FileError::RootNotModel));
            }
            let mut next_id = self.next_id;
            let mut components = BTreeMap::new();
            let mut names = std::collections::HashSet::new();
            let mut unique = |base: &str| {
                let name = (1..).map(|i| if i == 1 { base.to_owned() } else { format!("{base}#{i}") }).find(|n| !names.contains(n));
                let name = name.unwrap_or_default();
                names.insert(name.clone());
                name
            };
            for d in self.definitions.values() {
                unique(&d.name);
            }
            for d in self.definitions.into_values() {
                let mut occurrences = Vec::with_capacity(d.entities.len());
                for e in d.entities {
                    occurrences.push(match e {
                        Entity::Instance { id, name, definition, transform } => {
                            Occurrence { id, name, component: definition, transform, material: None }
                        }
                        Entity::Solid { id, name, solid } => {
                            let part = Id(next_id);
                            next_id += 1;
                            let kind = ComponentKind::Part(Part::new(solid));
                            components.insert(part, Arc::new(Component { id: part, name: unique(&name), named: false, kind }));
                            Occurrence { id, name: String::new(), component: part, transform: DAffine3::IDENTITY, material: None }
                        }
                    });
                }
                let named = d.kind != Kind::Group;
                let kind = ComponentKind::Assembly(Assembly { occurrences });
                components.insert(d.id, Arc::new(Component { id: d.id, name: d.name, named, kind }));
            }
            Ok(super::Document { components, root: self.root, next_id, settings: self.settings, materials: Default::default() })
        }
    }
}

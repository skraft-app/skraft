//! The file format: versioned JSON of the document (docs/ARCHITECTURE.md §8).

use serde::{Deserialize, Serialize};

use crate::document::Document;
use crate::{Error, Result};

/// The `format` field of every SolidSketch file.
pub const FORMAT: &str = "solidsketch";
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
struct FileIn {
    document: Document,
}

impl Document {
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(&FileOut { format: FORMAT, version: VERSION, document: self })
            .map_err(|e| Error::File(e.to_string()))
    }

    pub fn from_json(text: &str) -> Result<Document> {
        let h: Header = serde_json::from_str(text).map_err(|e| Error::File(e.to_string()))?;
        if h.format != FORMAT {
            return Err(Error::File(format!("not a SolidSketch file ({})", h.format)));
        }
        if h.version > VERSION {
            return Err(Error::File(format!("file version {} is newer than this build ({VERSION})", h.version)));
        }
        let f: FileIn = serde_json::from_str(text).map_err(|e| Error::File(e.to_string()))?;
        f.document.check()?;
        Ok(f.document)
    }

    /// The structural invariants a file must keep: a root, known definitions, no cycle, ids
    /// unique and below `next_id`.
    fn check(&self) -> Result<()> {
        use crate::document::{DefinitionKind, Entity};
        let root = self.definition(self.root)?;
        if root.kind != DefinitionKind::Root {
            return Err(Error::File("the root is not the model".into()));
        }
        let mut seen = std::collections::HashSet::new();
        for d in self.definitions() {
            if d.id.0 >= self.next_id || !seen.insert(d.id) {
                return Err(Error::File(format!("bad definition id {}", d.id)));
            }
            for e in &d.entities {
                if e.id().0 >= self.next_id || !seen.insert(e.id()) {
                    return Err(Error::File(format!("bad entity id {}", e.id())));
                }
                if let Entity::Instance(i) = e {
                    let target = self.definition(i.definition)?;
                    if target.kind == DefinitionKind::Root || self.contains_definition(i.definition, d.id) {
                        return Err(Error::Cycle);
                    }
                }
            }
        }
        Ok(())
    }
}

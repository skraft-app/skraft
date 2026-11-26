//! The file format (docs/io/file-format.md): a ZIP holding a manifest, the document's structure
//! as readable JSON, its B-reps and its images, one entry each, named by their content.
//!
//! Version 1: the app is not published yet, the format changes without a new version and
//! without reading older files.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::sync::Arc;

use glam::{DMat3, DVec2, DVec3};
use serde::{Deserialize, Serialize};
use skraft_kernel::Solid;
use zip::write::SimpleFileOptions;

use crate::document::{ComponentKind, Document, Id};
use crate::{Error, FastMap, FastSet, FileError, Result};

/// The `format` field of every Skraft file.
pub const FORMAT: &str = "skraft";
/// The version this build writes, and the newest it reads.
pub const VERSION: u32 = 1;

/// The manifest: what the file is, read first.
const MANIFEST: &str = "manifest.json";
/// The document, its solids by the names of their entries.
const DOCUMENT: &str = "document.json";
/// The B-reps the document is made of, `<name>.json`…
const BREPS: &str = "breps/";
/// …and those computed from them, kept to open without computing them again: the parts'
/// shapes, the profiled bodies' and the groups' solids.
const CACHE: &str = "cache/";
/// The images of the materials' textures, `<name>.<png|jpg>` (docs/render/look.md#textures).
const TEXTURES: &str = "textures/";
/// How the file was last seen ([`FileView`]): absent when it holds nothing.
const VIEW: &str = "view.json";
/// The most [`VIEW`] may hold: past it, the view is ignored.
const MAX_VIEW: u64 = 1 << 20;
/// What the model looks like, a PNG for the start page's recent files (docs/ui/file-tabs.md):
/// absent when the app had none to give.
const THUMBNAIL: &str = "thumbnail.png";
/// The most [`THUMBNAIL`] may hold: past it, the file has no thumbnail.
const MAX_THUMBNAIL: u64 = 4 << 20;

/// What a file may expand to once decompressed (docs/io/file-format.md#untrusted-files): this many
/// times its size (real files stay under 10)…
const MAX_EXPANSION: u64 = 256;
/// …never less than this…
pub(crate) const MIN_EXPANDED: u64 = 64 << 20;
/// …nor more than this.
#[cfg(not(target_arch = "wasm32"))]
const MAX_EXPANDED: u64 = 4 << 30;
#[cfg(target_arch = "wasm32")]
const MAX_EXPANDED: u64 = 1 << 30;

/// An entry's bytes, taken from what the file may still expand to (`left`); `None` past it. Its
/// `declared` size is only a hint: a forged one neither reserves memory nor lets more through.
pub(crate) fn read_entry(entry: impl Read, declared: u64, left: &mut u64) -> std::io::Result<Option<Vec<u8>>> {
    if declared > *left {
        return Ok(None);
    }
    let mut out = Vec::with_capacity(declared.min(MIN_EXPANDED) as usize);
    entry.take(*left + 1).read_to_end(&mut out)?;
    let read = out.len() as u64;
    if read > *left {
        return Ok(None);
    }
    *left -= read;
    Ok(Some(out))
}

/// How a file was last seen, not part of the document: written beside it, never in its
/// history — changing it neither modifies the document nor is undone (docs/io/file-format.md).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FileView {
    /// The workspace shown (an id of the app's; none, or one the app does not list: its first,
    /// decision 0047).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    /// The 3D view's camera; none: the whole model framed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera: Option<SeenCamera>,
    /// The drawing's board; none: every sheet framed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub board: Option<SeenBoard>,
}

/// The 3D camera as a file keeps it. The file is untrusted: the editor checks it when it takes it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeenCamera {
    pub eye: DVec3,
    pub yaw: f64,
    pub pitch: f64,
    pub projection: SeenProjection,
    pub focus: f64,
    pub frame: DMat3,
}

/// A [`SeenCamera`]'s projection.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeenProjection {
    /// Vertical field of view, radians.
    Perspective { fov_y: f64 },
    /// Visible height of the view, mm.
    Parallel { height: f64 },
}

/// The drawing's board as a file keeps it: the sheet active and how the board is seen.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeenBoard {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<Id>,
    /// The board point at the stage's centre, mm.
    pub pan: DVec2,
    /// Points per millimetre of paper.
    pub zoom: f64,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    content: Content,
    /// The app that wrote it (« Skraft 0.1.0 »), for information.
    #[serde(default)]
    generator: String,
}

/// What a file holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Content {
    /// A model, opened in a tab (`.skraft`).
    Model,
    /// Materials alone, imported into a model (`.skraftmat`): a document without geometry
    /// ([`Document::materials_fragment`]).
    Materials,
    /// A template (`.skrafttpl`, docs/ui/drawing.md#templates): a document holding only a
    /// drawing — a presentation, its sheets and compositions without views, their images
    /// ([`Document::template_of`]).
    Template,
}

/// The document as one JSON ([`Document::to_json`]).
#[derive(Serialize)]
struct JsonOut<'a> {
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
struct JsonIn<D> {
    document: D,
}

fn file_err(e: impl std::fmt::Display) -> Error {
    Error::File(FileError::Unreadable(e.to_string()))
}

/// Pretty JSON with its arrays of plain values on one line (`"transform": [1.0, 0.0, …]`, not
/// a line per number): what the indentation leaves of them once their spaces are dropped,
/// strings left as they are.
fn readable(pretty: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pretty.len());
    let (mut i, mut in_string) = (0, false);
    while i < pretty.len() {
        let b = pretty[i];
        if in_string {
            out.push(b);
            match b {
                b'\\' => {
                    out.extend(pretty.get(i + 1));
                    i += 1;
                }
                b'"' => in_string = false,
                _ => {}
            }
        } else if b == b'"' {
            in_string = true;
            out.push(b);
        } else if b == b'[' {
            // Its plain values: up to its end, before any string, array or object.
            let inner = pretty[i + 1..].iter().position(|c| matches!(c, b']' | b'[' | b'{' | b'"'));
            match inner {
                Some(n) if pretty[i + 1 + n] == b']' && pretty[i + 1..i + 1 + n].iter().any(|c| !c.is_ascii_whitespace()) => {
                    out.push(b'[');
                    let values = pretty[i + 1..i + 1 + n].split(|c| *c == b',').map(|v| v.trim_ascii());
                    for (k, v) in values.enumerate() {
                        if k > 0 {
                            out.extend_from_slice(b", ");
                        }
                        out.extend_from_slice(v);
                    }
                    out.push(b']');
                    i += n + 1;
                }
                _ => out.push(b),
            }
        } else {
            out.push(b);
        }
        i += 1;
    }
    out
}

fn check_header(format: &str, version: u32) -> Result<()> {
    if format != FORMAT {
        return Err(Error::File(FileError::NotSkraft(format.to_owned())));
    }
    if version > VERSION {
        return Err(Error::File(FileError::TooNew(version)));
    }
    Ok(())
}

impl Document {
    /// The document as a `.skraft` file. The same document gives the same bytes (no dates).
    pub fn to_file(&self) -> Result<Vec<u8>> {
        self.to_file_seen(&FileView::default())
    }

    /// The same, with how it was last seen ([`FileView`]; its entry left out when it holds
    /// nothing).
    pub fn to_file_seen(&self, view: &FileView) -> Result<Vec<u8>> {
        self.to_file_pictured(view, None)
    }

    /// The same, with a picture of the model (a PNG) for the start page.
    pub fn to_file_pictured(&self, view: &FileView, thumbnail: Option<&[u8]>) -> Result<Vec<u8>> {
        let view = (*view != FileView::default()).then_some(view);
        self.write(Content::Model, view, thumbnail)
    }

    /// The materials `ids` (all when empty) as a `.skraftmat` file: their textures' images,
    /// the definitions of the properties with them.
    pub fn materials_file(&self, ids: &[Id]) -> Result<Vec<u8>> {
        self.materials_fragment(ids)?.write(Content::Materials, None, None)
    }

    /// The presentation `presentation` as a template file (`.skrafttpl`,
    /// [`Document::template_of`]).
    pub fn template_file(&self, presentation: Id) -> Result<Vec<u8>> {
        self.template_of(presentation)?.write(Content::Template, None, None)
    }

    fn write(&self, content: Content, view: Option<&FileView>, thumbnail: Option<&[u8]>) -> Result<Vec<u8>> {
        let scope = solids::writing(self.source_solids());
        let document = readable(&serde_json::to_vec_pretty(self).map_err(file_err)?);
        let entries = scope.finish();
        let manifest =
            Manifest { format: FORMAT.to_owned(), version: VERSION, content, generator: format!("Skraft {}", env!("CARGO_PKG_VERSION")) };
        let manifest = serde_json::to_vec_pretty(&manifest).map_err(file_err)?;
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut put = |name: &str, bytes: &[u8]| -> Result<()> {
            zip.start_file(name, options).map_err(file_err)?;
            zip.write_all(bytes).map_err(file_err)
        };
        put(MANIFEST, &manifest)?;
        put(DOCUMENT, &document)?;
        if let Some(view) = view {
            put(VIEW, &serde_json::to_vec_pretty(view).map_err(file_err)?)?;
        }
        for (dir, map) in [(BREPS, &entries.sources), (CACHE, &entries.caches)] {
            for (name, json) in map {
                put(&format!("{dir}{name}.json"), json)?;
            }
        }
        // Stored as they are: a PNG or a JPEG is compressed already.
        let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, bytes) in &entries.images {
            zip.start_file(format!("{TEXTURES}{name}"), stored).map_err(file_err)?;
            zip.write_all(bytes).map_err(file_err)?;
        }
        if let Some(png) = thumbnail {
            zip.start_file(THUMBNAIL, stored).map_err(file_err)?;
            zip.write_all(png).map_err(file_err)?;
        }
        Ok(zip.finish().map_err(file_err)?.into_inner())
    }

    /// The model of a `.skraft` file (a `.skraftmat` is refused: [`Document::read_file`]).
    pub fn from_file(bytes: &[u8]) -> Result<Document> {
        match Document::read_file(bytes)? {
            (doc, Content::Model) => Ok(doc),
            (_, Content::Materials) => Err(Error::File(FileError::MaterialsOnly)),
            (_, Content::Template) => Err(Error::File(FileError::TemplateOnly)),
        }
    }

    /// How a `.skraft` file was last seen: the default when it says nothing, or nothing
    /// readable (a view state never fails an opening).
    pub fn file_view(bytes: &[u8]) -> FileView {
        let Ok(mut zip) = zip::ZipArchive::new(Cursor::new(bytes)) else { return FileView::default() };
        let Ok(entry) = zip.by_name(VIEW) else { return FileView::default() };
        let mut out = Vec::new();
        match entry.take(MAX_VIEW + 1).read_to_end(&mut out) {
            Ok(read) if read as u64 <= MAX_VIEW => serde_json::from_slice(&out).unwrap_or_default(),
            _ => FileView::default(),
        }
    }

    /// The picture of the model a `.skraft` file carries (a PNG), read from it alone: the
    /// file's other entries are not read. None when it has none, or one too large.
    pub fn file_thumbnail(file: impl Read + std::io::Seek) -> Option<Vec<u8>> {
        let mut zip = zip::ZipArchive::new(file).ok()?;
        let entry = zip.by_name(THUMBNAIL).ok()?;
        let mut out = Vec::new();
        match entry.take(MAX_THUMBNAIL + 1).read_to_end(&mut out) {
            Ok(read) if read as u64 <= MAX_THUMBNAIL => Some(out),
            _ => None,
        }
    }

    /// The document of a `.skraft` or `.skraftmat` file, and what it holds.
    pub fn read_file(bytes: &[u8]) -> Result<(Document, Content)> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| Error::File(FileError::NotZip))?;
        let paths: Vec<String> =
            zip.file_names().map(|n| n.map(|n| n.into_owned())).collect::<std::result::Result<_, _>>().map_err(file_err)?;
        let mut left = (bytes.len() as u64).saturating_mul(MAX_EXPANSION).clamp(MIN_EXPANDED, MAX_EXPANDED);
        let mut read = |name: &str| -> Result<Vec<u8>> {
            let entry = zip.by_name(name).map_err(|_| Error::File(FileError::MissingEntry(name.to_owned())))?;
            let declared = entry.size();
            read_entry(entry, declared, &mut left).map_err(|e| file_err(format!("{name}: {e}")))?.ok_or(Error::File(FileError::TooLarge))
        };
        let manifest: Manifest = serde_json::from_slice(&read(MANIFEST)?).map_err(|e| file_err(format!("{MANIFEST}: {e}")))?;
        check_header(&manifest.format, manifest.version)?;
        let document = read(DOCUMENT)?;
        let mut solids = FastMap::default();
        let mut images = FastMap::default();
        for path in paths {
            if let Some(name) = path.strip_prefix(TEXTURES).and_then(|n| n.rsplit_once('.')).map(|(id, _)| id.to_owned()) {
                images.insert(name, Arc::<[u8]>::from(read(&path)?));
                continue;
            }
            let name = [BREPS, CACHE].iter().find_map(|dir| path.strip_prefix(dir)).and_then(|n| n.strip_suffix(".json"));
            let Some(name) = name.map(str::to_owned) else { continue };
            let solid: Solid = serde_json::from_slice(&read(&path)?).map_err(|e| file_err(format!("{path}: {e}")))?;
            solids.insert(name, Arc::new(solid));
        }
        let scope = solids::reading(solids, images);
        let doc = serde_json::from_slice::<Document>(&document).map_err(|e| file_err(format!("{DOCUMENT}: {e}")));
        drop(scope);
        Ok((doc?.settled()?, manifest.content))
    }

    /// The document as one JSON, its B-reps inline: what a recording of the journal starts
    /// from (docs/control-protocol.md), and the tests'.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(&JsonOut { format: FORMAT, version: VERSION, document: self }).map_err(file_err)
    }

    /// The document of [`Document::to_json`]'s text.
    pub fn from_json(text: &str) -> Result<Document> {
        let h: Header = serde_json::from_str(text).map_err(file_err)?;
        check_header(&h.format, h.version)?;
        serde_json::from_str::<JsonIn<Document>>(text).map_err(file_err)?.document.settled()
    }

    /// A document just read: checked, its automatic names settled.
    fn settled(mut self) -> Result<Document> {
        self.check()?;
        self.check_props()?;
        self.settle_link_names();
        self.settle_names(None);
        Ok(self)
    }

    /// The solids the document is made of, by address: its leaf bodies' that no profile builds.
    /// The others are computed from them (parts' shapes, profiled bodies', groups'): caches.
    fn source_solids(&self) -> FastSet<*const Solid> {
        let mut out = FastSet::default();
        for c in self.components.values() {
            if let ComponentKind::Part(p) = &c.kind {
                for d in p.defs.iter().filter(|d| d.profile.is_none() && d.group.is_none()) {
                    out.insert(Arc::as_ptr(&d.solid));
                }
            }
        }
        out
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
        for (id, m) in &self.materials {
            if id.0 >= self.next_id {
                return Err(Error::File(FileError::BadMaterialId(*id)));
            }
            if m.texture.as_ref().is_some_and(|t| !t.is_valid() || !self.assets.contains_key(&t.image)) {
                return Err(Error::File(FileError::BadTexture(*id)));
            }
        }
        // The drawing (docs/ui/drawing.md): ids fresh, images present, views resolved, what
        // hangs on a view beside it, occurrences of known compositions, no composition holding itself, a
        // paper's corner only on a sheet.
        let d = &self.drawing;
        for p in &d.presentations {
            fresh(p.id, FileError::BadDrawing)?;
            for s in &p.sheets {
                fresh(s.id, FileError::BadDrawing)?;
            }
        }
        for (key, b) in &d.compositions {
            if *key != b.id {
                return Err(Error::File(FileError::BadDrawing(b.id)));
            }
            fresh(b.id, FileError::BadDrawing)?;
        }
        for (container, elements) in d.containers() {
            for e in elements {
                fresh(e.id, FileError::BadDrawing)?;
                let beside = e.kind.hung_on().is_none_or(|v| elements.iter().any(|x| x.id == v));
                let fine = beside
                    && match &e.kind {
                        crate::ElementKind::Image { asset, .. } => self.assets.contains_key(asset),
                        crate::ElementKind::View(_) => d.resolve(e.id).is_some(),
                        crate::ElementKind::Occurrence(i) => {
                            d.composition(i.composition).is_some()
                                && match container {
                                    crate::Container::Sheet(_) => true,
                                    crate::Container::Composition(b) => i.place.corner.is_none() && !d.reaches(i.composition, b),
                                }
                        }
                        _ => true,
                    };
                if !fine {
                    return Err(Error::File(FileError::BadDrawing(e.id)));
                }
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
                        if d.id.0 >= self.next_id || !here.insert(d.id) || d.imprint.as_ref().is_some_and(|r| !r.clearance.is_valid()) {
                            return Err(Error::File(FileError::BadBodyId(d.id)));
                        }
                    }
                    // A linked body is a machining (docs/model/bodies.md#links).
                    for b in p.all_bodies() {
                        let linked_off = |d: &crate::BodyDef| d.source == crate::BodySource::Linked && b.sign != crate::Sign::Negative;
                        if b.id.0 >= self.next_id || !here.insert(b.id) || p.def(b.def).is_none_or(linked_off) {
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
                    // A link whose path no longer leads anywhere is broken, not malformed
                    // (docs/decisions/0055-linked-bodies-through-imprints.md).
                    for l in a.links() {
                        if l.id.0 >= self.next_id
                            || !here.insert(l.id)
                            || l.source.is_empty()
                            || l.receiver.is_empty()
                            || !crate::ops::is_rigid(&l.seat)
                        {
                            return Err(Error::File(FileError::BadLink(l.id)));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// `#[serde(with)]` for a field of a solid: [`solids`].
pub(crate) mod solid {
    pub(crate) use super::solids::{de as deserialize, ser as serialize};
}

/// `#[serde(with)]` for a field of an optional solid: [`solids`].
pub(crate) mod solid_opt {
    use std::sync::Arc;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use skraft_kernel::Solid;

    struct Out<'a>(&'a Arc<Solid>);

    impl Serialize for Out<'_> {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            super::solids::ser(self.0, s)
        }
    }

    struct In(Arc<Solid>);

    impl<'de> Deserialize<'de> for In {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            super::solids::de(d).map(In)
        }
    }

    pub(crate) fn serialize<S: Serializer>(solid: &Option<Arc<Solid>>, s: S) -> Result<S::Ok, S::Error> {
        solid.as_ref().map(Out).serialize(s)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Arc<Solid>>, D::Error> {
        Ok(Option::<In>::deserialize(d)?.map(|i| i.0))
    }
}

/// How the document's solids serialise: inline, as their record — or, while a file is written
/// or read ([`writing`], [`reading`]), as the name of their entry, 128 bits of the BLAKE3 hash
/// of their record's JSON: the same solid twice, one entry. The state lives in a thread-local
/// for the time of one `serde_json` call, serde passing nothing down to a field's serialiser.
pub(crate) mod solids {
    use std::cell::RefCell;
    use std::fmt;

    use serde::de::{self, MapAccess, SeqAccess, Visitor};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::{Arc, BTreeMap, FastMap, FastSet, Solid};

    /// An entry's name: hexadecimal digits of the hash.
    const NAME_LEN: usize = 32;

    enum State {
        Writing(Entries),
        /// The solids and the images (their bytes) by their entries' names.
        Reading(FastMap<String, Arc<Solid>>, FastMap<String, Arc<[u8]>>),
    }

    thread_local! {
        static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    }

    /// The entries of a file being written: the records' JSON by name.
    #[derive(Default)]
    pub(super) struct Entries {
        /// The solids the document is made of, by address; the others are caches.
        made_of: FastSet<*const Solid>,
        /// The names given, by address: a solid shared, hashed once.
        names: FastMap<*const Solid, String>,
        pub(super) sources: BTreeMap<String, Vec<u8>>,
        /// Only those not among the sources: a cache equal to a source is that source.
        pub(super) caches: BTreeMap<String, Vec<u8>>,
        /// The images, by their entries' names (`<id>.<ext>`).
        pub(super) images: BTreeMap<String, Arc<[u8]>>,
    }

    impl Entries {
        fn name(&mut self, solid: &Arc<Solid>) -> serde_json::Result<String> {
            let at = Arc::as_ptr(solid);
            if let Some(name) = self.names.get(&at) {
                return Ok(name.clone());
            }
            let json = serde_json::to_vec(&**solid)?;
            let name = blake3::hash(&json).to_hex()[..NAME_LEN].to_owned();
            if self.made_of.contains(&at) {
                self.caches.remove(&name);
                self.sources.insert(name.clone(), json);
            } else if !self.sources.contains_key(&name) {
                self.caches.insert(name.clone(), json);
            }
            self.names.insert(at, name.clone());
            Ok(name)
        }
    }

    /// The solids written as entries until [`Writing::finish`]; `made_of`: those the
    /// document is made of, by address.
    pub(super) fn writing(made_of: FastSet<*const Solid>) -> Writing {
        STATE.set(Some(State::Writing(Entries { made_of, ..Entries::default() })));
        Writing
    }

    pub(super) struct Writing;

    impl Writing {
        /// The entries written since.
        pub(super) fn finish(self) -> Entries {
            match STATE.take() {
                Some(State::Writing(e)) => e,
                _ => Entries::default(),
            }
        }
    }

    impl Drop for Writing {
        fn drop(&mut self) {
            STATE.set(None);
        }
    }

    /// The solids and the images read by their entries' names, from `solids` and `images`,
    /// until dropped.
    pub(super) fn reading(solids: FastMap<String, Arc<Solid>>, images: FastMap<String, Arc<[u8]>>) -> Reading {
        STATE.set(Some(State::Reading(solids, images)));
        Reading
    }

    /// `#[serde(with)]` for the document's images ([`crate::Asset`]): inline, each its kind and
    /// its bytes in base64; while a file is written or read, its kind, its bytes an entry
    /// `textures/<id>.<ext>`.
    pub(crate) mod images {
        use super::*;
        use crate::assets::{Asset, ImageKind};

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Stored {
            Entry(ImageKind),
            Inline(Asset),
        }

        pub(crate) fn serialize<S: Serializer>(assets: &BTreeMap<String, Asset>, s: S) -> Result<S::Ok, S::Error> {
            let writing = STATE.with_borrow_mut(|state| match state {
                Some(State::Writing(entries)) => {
                    for (id, a) in assets {
                        entries.images.insert(format!("{id}.{}", a.kind.extension()), a.bytes.clone());
                    }
                    true
                }
                _ => false,
            });
            match writing {
                true => s.collect_map(assets.iter().map(|(id, a)| (id, a.kind))),
                false => assets.serialize(s),
            }
        }

        pub(crate) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<BTreeMap<String, Asset>, D::Error> {
            let stored = BTreeMap::<String, Stored>::deserialize(d)?;
            stored
                .into_iter()
                .map(|(id, s)| {
                    let asset = match s {
                        Stored::Inline(a) => a,
                        Stored::Entry(kind) => {
                            let bytes = STATE.with_borrow(|state| match state {
                                Some(State::Reading(_, images)) => images.get(&id).cloned(),
                                _ => None,
                            });
                            let bytes = bytes.ok_or_else(|| de::Error::custom(format!("image {id} missing")))?;
                            Asset::new(bytes)
                                .filter(|a| a.kind == kind)
                                .ok_or_else(|| de::Error::custom(format!("image {id} unreadable")))?
                        }
                    };
                    Ok((id, asset))
                })
                .collect()
        }
    }

    pub(super) struct Reading;

    impl Drop for Reading {
        fn drop(&mut self) {
            STATE.set(None);
        }
    }

    pub(crate) fn ser<S: Serializer>(solid: &Arc<Solid>, s: S) -> Result<S::Ok, S::Error> {
        let name = STATE.with_borrow_mut(|state| match state {
            Some(State::Writing(entries)) => Some(entries.name(solid)),
            _ => None,
        });
        match name {
            Some(name) => s.serialize_str(&name.map_err(serde::ser::Error::custom)?),
            None => solid.serialize(s),
        }
    }

    pub(crate) fn de<'de, D: Deserializer<'de>>(d: D) -> Result<Arc<Solid>, D::Error> {
        d.deserialize_any(SolidVisitor)
    }

    /// A record inline, or an entry's name while reading a file.
    struct SolidVisitor;

    impl<'de> Visitor<'de> for SolidVisitor {
        type Value = Arc<Solid>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a B-rep record or the name of its entry")
        }

        fn visit_str<E: de::Error>(self, name: &str) -> Result<Arc<Solid>, E> {
            STATE
                .with_borrow(|state| match state {
                    Some(State::Reading(solids, _)) => solids.get(name).cloned(),
                    _ => None,
                })
                .ok_or_else(|| E::custom(format!("B-rep {name} missing")))
        }

        fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Arc<Solid>, A::Error> {
            Solid::deserialize(de::value::MapAccessDeserializer::new(map)).map(Arc::new)
        }

        fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Arc<Solid>, A::Error> {
            Solid::deserialize(de::value::SeqAccessDeserializer::new(seq)).map(Arc::new)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn readable_keeps_plain_arrays_on_one_line_and_strings_as_they_are() {
        let value = serde_json::json!({
            "t": [1.0, -2.5e-7, 3],
            "name": "a [1, 2] \"b\\\" [",
            "list": [[1, 2], {"p": [0, 0]}, "s"],
            "none": [],
            "flags": [true, null],
        });
        let pretty = serde_json::to_vec_pretty(&value).unwrap();
        let out = String::from_utf8(super::readable(&pretty)).unwrap();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&out).unwrap(), value);
        assert!(out.contains(r#""t": [1.0, -2.5e-7, 3]"#), "{out}");
        assert!(out.contains(r#""p": [0, 0]"#), "{out}");
        assert!(out.contains(r#""flags": [true, null]"#), "{out}");
        assert!(out.contains(r#""name": "a [1, 2] \"b\\\" [""#), "{out}");
        assert!(out.contains("\"none\": []"), "{out}");
    }
}

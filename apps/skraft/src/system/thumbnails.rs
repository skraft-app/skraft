//! The components' thumbnails (docs/ui/components-library.md#thumbnails): one picture per part or
//! assembly, drawn off screen by the viewport's renderer ([`skraft_render::Renderer::render_picture`]),
//! kept per document under a key of what it shows. The library asks for the ones it
//! shows ([`Thumbnails::get`]); the viewport draws those asked for at its next frame, a few
//! within a time budget ([`crate::workspaces::common::viewport::Viewport`]), the nature's icon standing in meanwhile.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use glam::DAffine3;
use skraft_model::{Document, Id};
use skraft_tools::Editor;

/// A thumbnail's side in the grid, in points.
pub const TILE: f32 = 64.0;
/// The most thumbnails kept, all documents together: the least recently shown go first.
pub const CAP: usize = 300;
/// The most placements an assembly's thumbnail draws (a model of many copies: the first ones).
const MAX_PLACEMENTS: usize = 5000;

/// A part mesh and its placements, each with the material it shows.
pub type PartPlaces = (Id, Vec<(DAffine3, Option<Id>)>);

/// What a component's thumbnail draws, in its own axes: each part mesh, at its placements
/// (with the material each one shows), and a hash of all of it.
pub struct Content {
    pub parts: Vec<PartPlaces>,
    hash: u64,
}

impl Content {
    /// The whole model: the root's content, as placed in the world.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn model(doc: &Document) -> Content {
        Content::of(doc, doc.root())
    }

    fn of(doc: &Document, component: Id) -> Content {
        let mut parts: Vec<PartPlaces> = Vec::new();
        let mut count = 0;
        // Each part in its own material.
        fn walk(doc: &Document, c: Id, t: DAffine3, parts: &mut Vec<PartPlaces>, count: &mut usize, depth: usize) {
            if *count >= MAX_PLACEMENTS || depth > skraft_model::MAX_DEPTH {
                return;
            }
            if let Ok(p) = doc.part(c) {
                *count += 1;
                match parts.iter_mut().find(|(p, _)| *p == c) {
                    Some((_, places)) => places.push((t, p.material)),
                    None => parts.push((c, vec![(t, p.material)])),
                }
                return;
            }
            let Ok(a) = doc.assembly(c) else { return };
            for o in a.occurrences().iter().filter(|o| !o.hidden) {
                walk(doc, o.component, t * o.transform, parts, count, depth + 1);
            }
        }
        walk(doc, component, DAffine3::IDENTITY, &mut parts, &mut count, 0);
        let mut h = std::hash::DefaultHasher::new();
        for (p, places) in &parts {
            p.0.hash(&mut h);
            for (t, m) in places {
                for v in t.to_cols_array() {
                    v.to_bits().hash(&mut h);
                }
                m.hash(&mut h);
                // A material's colour changed: drawn again.
                m.and_then(|m| doc.material(m).ok()).map(|m| m.color).hash(&mut h);
            }
        }
        Content { parts, hash: h.finish() }
    }
}

/// A thumbnail drawn.
struct Thumb {
    picture: skraft_render::Picture,
    texture: egui::TextureId,
    key: u64,
    /// The last tick it was shown at.
    used: u64,
}

/// A thumbnail asked for, to draw.
pub struct Wanted {
    pub doc: u64,
    pub component: Id,
    pub key: u64,
    pub content: Rc<Content>,
}

#[derive(Default)]
pub struct Thumbnails {
    /// What each component's thumbnail draws, made again when its document changes (its
    /// history's revision).
    contents: HashMap<(u64, Id), (u64, Rc<Content>)>,
    shown: HashMap<(u64, Id), Thumb>,
    wanted: Vec<Wanted>,
    /// The documents closed: their thumbnails freed at the next draw.
    dropped: Vec<u64>,
    tick: u64,
}

impl Thumbnails {
    /// The thumbnail of `component` (document `doc`, shown by `editor`) at `px` pixels, if
    /// drawn: the one drawn before while a new one is due. Asks for it when it is missing or
    /// out of date and its meshes are all there (a component placed nowhere has none in the
    /// scene: its icon stays).
    pub fn get(&mut self, doc: u64, editor: &Editor, component: Id, dark: bool, px: u32) -> Option<egui::TextureId> {
        let revision = editor.history().revision();
        let content = match self.contents.get(&(doc, component)) {
            Some((r, c)) if *r == revision => c.clone(),
            _ => {
                let c = Rc::new(Content::of(editor.document(), component));
                self.contents.insert((doc, component), (revision, c.clone()));
                c
            }
        };
        let key = key(editor, &content, dark, px);
        let shown = self.shown.get_mut(&(doc, component));
        let current = shown.as_ref().is_some_and(|t| Some(t.key) == key);
        let texture = shown.map(|t| {
            t.used = self.tick;
            t.texture
        });
        if let Some(key) = key.filter(|_| !current)
            && !self.wanted.iter().any(|w| w.doc == doc && w.component == component)
        {
            self.wanted.push(Wanted { doc, component, key, content });
        }
        texture
    }

    /// Are thumbnails waiting to be drawn?
    pub fn pending(&self) -> bool {
        !self.wanted.is_empty()
    }

    /// The thumbnails asked for, to draw now (taken out).
    pub fn take_wanted(&mut self) -> Vec<Wanted> {
        std::mem::take(&mut self.wanted)
    }

    /// Gives back those not drawn this frame (the time budget spent).
    pub fn defer(&mut self, rest: Vec<Wanted>) {
        self.wanted = rest;
    }

    /// The picture to draw `w` into: its own again when of `size`, else a new one.
    pub fn picture_for(&mut self, w: &Wanted, size: [u32; 2]) -> Option<&skraft_render::Picture> {
        self.shown.get(&(w.doc, w.component)).filter(|t| t.picture.size() == size).map(|t| &t.picture)
    }

    /// `w` drawn into `picture`, shown as `texture`. Returns the texture it replaces, to free.
    pub fn drawn(&mut self, w: &Wanted, picture: skraft_render::Picture, texture: egui::TextureId) -> Option<egui::TextureId> {
        let thumb = Thumb { picture, texture, key: w.key, used: self.tick };
        self.shown.insert((w.doc, w.component), thumb).map(|t| t.texture).filter(|t| *t != texture)
    }

    /// `w` drawn again into its own picture.
    pub fn redrawn(&mut self, w: &Wanted) {
        if let Some(t) = self.shown.get_mut(&(w.doc, w.component)) {
            t.key = w.key;
        }
    }

    /// The textures to free: the closed documents', and the least recently shown past
    /// [`CAP`]. Call once per frame, with the textures still registered.
    pub fn evict(&mut self) -> Vec<egui::TextureId> {
        self.tick += 1;
        let mut freed = Vec::new();
        if !self.dropped.is_empty() {
            let dropped = std::mem::take(&mut self.dropped);
            self.shown.retain(|(doc, _), t| {
                let keep = !dropped.contains(doc);
                if !keep {
                    freed.push(t.texture);
                }
                keep
            });
            self.contents.retain(|(doc, _), _| !dropped.contains(doc));
        }
        if self.shown.len() > CAP {
            let mut by_age: Vec<((u64, Id), u64)> = self.shown.iter().map(|(k, t)| (*k, t.used)).collect();
            by_age.sort_by_key(|(_, used)| *used);
            for (k, _) in by_age.into_iter().take(self.shown.len() - CAP) {
                if let Some(t) = self.shown.remove(&k) {
                    freed.push(t.texture);
                }
            }
        }
        freed
    }

    /// Document `doc` closed: its thumbnails go.
    pub fn drop_document(&mut self, doc: u64) {
        self.dropped.push(doc);
        self.wanted.retain(|w| w.doc != doc);
    }
}

/// The key of a thumbnail: what it draws, its meshes' revisions, the theme, its size; none
/// while a mesh it needs is missing.
fn key(editor: &Editor, content: &Content, dark: bool, px: u32) -> Option<u64> {
    if content.parts.is_empty() {
        return None;
    }
    let scene = editor.scene();
    let mut h = std::hash::DefaultHasher::new();
    content.hash.hash(&mut h);
    for (p, _) in &content.parts {
        scene.definition_mesh(*p)?.revision.hash(&mut h);
    }
    (dark, px).hash(&mut h);
    Some(h.finish())
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use skraft_kernel::Inline;
    use skraft_model::{EntityPath, Operation, Primitive};

    use super::*;

    fn add(e: &mut Editor, origin: DVec3) -> Id {
        let root = e.document().root();
        let p = Primitive::Box { origin, size: DVec3::splat(10.0) };
        e.apply(Operation::AddPrimitive { context: root, primitive: p, placement: DAffine3::IDENTITY, material: None })
            .unwrap()
            .unwrap()
            .created[0]
    }

    #[test]
    fn a_thumbnail_draws_its_parts_where_its_assembly_places_them() {
        let mut e = Editor::new(Document::new(), Box::new(Inline::default()));
        let root = e.document().root();
        let a = add(&mut e, DVec3::ZERO);
        let b = add(&mut e, DVec3::new(100.0, 0.0, 0.0));
        let g = e.apply(Operation::MakeAssembly { context: root, entities: vec![a, b], name: None }).unwrap().unwrap().created[0];
        let doc = e.document();
        let asm = doc.resolve(&EntityPath::root(g)).unwrap().component;
        let pa = doc.resolve(&EntityPath::new(vec![g], a)).unwrap().component;

        // A part: itself, once, in its own axes.
        let part = Content::of(doc, pa);
        assert_eq!(part.parts.len(), 1);
        assert_eq!(part.parts[0].1, vec![(DAffine3::IDENTITY, None)]);

        // An assembly: its parts as placed in it.
        let content = Content::of(doc, asm);
        assert_eq!(content.parts.len(), 2);
        let at = |c: &Content, k: usize| c.parts[k].1[0].0.translation;
        assert!((at(&content, 1) - at(&content, 0) - DVec3::new(100.0, 0.0, 0.0)).length() < 1e-9);

        // Its parts painted oak: drawn in oak; a hidden part left out.
        let oak = e.apply(Operation::AddMaterial { name: "Chêne".into(), color: [180, 140, 90] }).unwrap().unwrap().material.unwrap();
        let targets = vec![skraft_model::EntityPath::root(g)];
        e.apply(Operation::PaintParts { targets, material: Some(oak), scope: skraft_model::Scope::All }).unwrap();
        let paint = Operation::SetHidden { context: Id(0), entities: vec![b], hidden: true };
        e.apply(Operation::At { parent: vec![g], unique: false, op: Box::new(paint) }).unwrap();
        let painted = Content::of(e.document(), asm);
        assert_eq!(painted.parts.len(), 1, "the hidden part left out");
        assert_eq!(painted.parts[0].1[0].1, Some(oak));
        assert_ne!(painted.hash, content.hash, "drawn again");
    }
}

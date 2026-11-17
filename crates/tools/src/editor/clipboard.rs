//! Copy, cut and paste (docs/ARCHITECTURE.md §6 decies): the selection copied as a fragment,
//! pasted again into this document or another, in place or at the cursor. The clipboard
//! itself is the app's (one for every document); the editor makes and places what it holds.

use std::sync::Arc;

use glam::{DAffine3, DVec3};
use solidsketch_model::{BodyFragment, Document, GuideShape, Operation};

use super::{After, Editor};
use crate::ToolKind;
use crate::scene::{ContextIndex, transform_bounds};
use crate::tools::{self, MoveTool};

/// What the clipboard holds.
#[derive(Clone, Debug)]
pub struct Clip {
    pub content: Content,
    /// Points bounding it, in the frame of its content (world for occurrences, the part's
    /// axes for machinings): the corners of its solids' boxes, a point of each guide. Pasted
    /// at the cursor, it is held by the low corner of their box.
    hull: Vec<DVec3>,
}

impl Clip {
    /// `content`, bounded by the points `hull` (in its frame).
    pub fn new(content: Content, hull: Vec<DVec3>) -> Self {
        Clip { content, hull }
    }
}

/// What was copied.
#[derive(Clone, Debug)]
pub enum Content {
    /// Occurrences and guides of an assembly, in world axes.
    Entities(Arc<Document>),
    /// Machinings and guides of an open part, in its axes.
    Machinings(Arc<BodyFragment>),
}

/// A paste carried at the cursor (§6 decies): given up, it is undone.
pub(super) struct Carry {
    /// The history's revision with the paste done in place.
    revision: u64,
    /// The tool to go back to once it is placed or given up.
    previous: ToolKind,
}

impl Editor {
    /// Can the selection be copied now (nothing under way, the tool at its first step)?
    pub fn can_copy(&self) -> bool {
        self.anything_selected() && self.idle()
    }

    /// The selection, copied: an open part's machinings and guides, else the context's
    /// occurrences and guides.
    pub fn copy(&self) -> Option<Clip> {
        if !self.can_copy() {
            return None;
        }
        let doc = self.document();
        // Parts at any depth (« Parts », §6 sedecies): each from its own assembly.
        if !self.deep.is_empty() {
            let f = doc.paths_fragment(&self.deep).ok()?;
            let mut hull = Vec::new();
            for p in &self.deep {
                let Some(ctx) = ContextIndex::new(doc, &p.instances) else { continue };
                let b = self.content_bounds(&ctx, p.entity, DAffine3::IDENTITY, 0.0);
                hull.extend(b.map(|b| b.corners()).into_iter().flatten());
            }
            return Some(Clip { content: Content::Entities(Arc::new(f)), hull });
        }
        match self.open_part_path() {
            Some(path) => {
                let f = doc.bodies_fragment(&path, &self.selection).ok()?;
                let mut hull = guide_points(&f.guides, &DAffine3::IDENTITY);
                for m in &f.bodies {
                    let Some(d) = f.defs.iter().find(|d| d.id == m.def) else { continue };
                    let b = self.scene.bounds_of(&d.solid).or_else(|| d.solid.vertex_bounds());
                    hull.extend(b.map(|b| box_corners(transform_bounds(&m.placement, b))).into_iter().flatten());
                }
                Some(Clip { content: Content::Machinings(Arc::new(f)), hull })
            }
            None => {
                let f = doc.entities_fragment(&self.context, &self.selection).ok()?;
                let root = f.component(f.root()).ok()?;
                let mut hull = guide_points(&root.guides.iter().map(|g| g.shape).collect::<Vec<_>>(), &DAffine3::IDENTITY);
                if let Some(ctx) = ContextIndex::new(doc, &self.context) {
                    for o in f.root_assembly().occurrences() {
                        let b = self.content_bounds(&ctx, o.id, DAffine3::IDENTITY, 0.0);
                        hull.extend(b.map(|b| b.corners()).into_iter().flatten());
                    }
                }
                Some(Clip { content: Content::Entities(Arc::new(f)), hull })
            }
        }
    }

    /// The selection copied, then erased.
    pub fn cut(&mut self) -> Option<Clip> {
        let clip = self.copy()?;
        self.delete_selection();
        Some(clip)
    }

    /// Can `clip` be pasted here now? Anywhere: occurrences pasted into an open part become
    /// bodies of it, bodies pasted out of one a new part (§6 decies, across the part's
    /// boundary).
    pub fn can_paste(&self, _clip: &Clip) -> bool {
        self.idle()
    }

    /// Pastes `clip` where it was copied from: in world space for occurrences, brought into
    /// the active context (into an open part, as its bodies); in the open part's axes for
    /// machinings (out of a part, a new part in the axes of the part copied from). With `reuse`, the
    /// document is the one it was copied from, its ids still naming the same things (the
    /// copies are then occurrences of the same parts). What is pasted is selected.
    pub fn paste_in_place(&mut self, clip: &Clip, reuse: bool) {
        if let Some(op) = self.paste_op(clip, reuse) {
            let _ = self.start(op, After::Select, false);
        }
    }

    /// Pastes `clip` at the cursor: pasted in place, then carried by the Move tool from the
    /// low corner of its box (in the axes), until a click puts it down — one step in the
    /// history — or Esc, Undo or another tool gives it up.
    pub fn paste(&mut self, clip: &Clip, reuse: bool) {
        let Some(op) = self.paste_op(clip, reuse) else { return };
        // The hull in the axes tools work in (the open part's for machinings pasted into one,
        // whose hull is in them already).
        let axes = self.axes_transform();
        let to_axes = match &clip.content {
            Content::Entities(_) => axes.inverse(),
            Content::Machinings(_) if self.open_part.is_some() => DAffine3::IDENTITY,
            Content::Machinings(f) => axes.inverse() * f.world,
        };
        let low = clip.hull.iter().map(|p| to_axes.transform_point3(*p)).reduce(DVec3::min);
        let after = match low {
            Some(low) => After::Carry(axes.transform_point3(low)),
            None => After::Select,
        };
        let _ = self.start(op, after, false);
    }

    /// The operation pasting `clip` in place, if it can be pasted here.
    fn paste_op(&mut self, clip: &Clip, reuse: bool) -> Option<Operation> {
        if !self.can_paste(clip) {
            return None;
        }
        let into = self.open_part_path();
        let op = match (&clip.content, into) {
            (Content::Entities(fragment), None) => Operation::Paste {
                context: self.context_definition(),
                fragment: fragment.clone(),
                transform: self.context_transform().inverse(),
                reuse,
            },
            (Content::Entities(fragment), Some(target)) => Operation::PasteParts {
                target,
                fragment: fragment.clone(),
                transform: self.axes_transform().inverse(),
                scope: self.part_scope,
            },
            (Content::Machinings(fragment), Some(target)) => {
                Operation::PasteBodies { target, fragment: fragment.clone(), transform: DAffine3::IDENTITY, scope: self.part_scope }
            }
            (Content::Machinings(fragment), None) => Operation::PasteAsPart {
                context: self.context_definition(),
                fragment: fragment.clone(),
                transform: self.context_transform().inverse(),
            },
        };
        Some(op)
    }

    /// The paste `op` done in place, what it made selected: carried from `from` by the Move
    /// tool.
    pub(super) fn carry(&mut self, from: DVec3, op: Operation) {
        if self.selection.is_empty() {
            return;
        }
        let previous = self.tool.kind();
        self.carry = Some(Carry { revision: self.history.revision(), previous });
        self.tool = Box::new(MoveTool::carrying(self.selection.clone(), from, op));
        self.clear_locks();
        self.reference = None;
    }

    /// Is a paste carried at the cursor?
    pub fn carrying(&self) -> bool {
        self.carry.is_some()
    }

    /// Once the paste carried is put down (or the tool left it), back to the tool before; given
    /// up — the paste in place still the last step —, it is undone.
    pub(super) fn settle_carry(&mut self, give_up: bool) {
        let Some(c) = &self.carry else { return };
        if !give_up && self.tool.kind() == ToolKind::Move && self.tool.busy() {
            return;
        }
        let previous = c.previous;
        if self.history.revision() == c.revision && !self.busy() {
            self.history.undo();
            self.clear_selection();
            self.after_change();
        }
        self.carry = None;
        self.tool = tools::make(previous);
        self.clear_locks();
    }

    /// Nothing under way and the tool at its first step.
    fn idle(&self) -> bool {
        !self.busy() && !self.tool.busy()
    }
}

/// The eight corners of an axis-aligned box.
fn box_corners((lo, hi): (DVec3, DVec3)) -> [DVec3; 8] {
    std::array::from_fn(|i| {
        DVec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z })
    })
}

/// A point of each guide (a line's origin, a circle's centre), placed by `t`.
fn guide_points(guides: &[GuideShape], t: &DAffine3) -> Vec<DVec3> {
    guides
        .iter()
        .map(|g| match *g {
            GuideShape::Line { origin, .. } => t.transform_point3(origin),
            GuideShape::Circle { center, .. } => t.transform_point3(center),
        })
        .collect()
}

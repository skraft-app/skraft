//! Erase (SketchUp's eraser): a click erases the entity of the active context under the
//! cursor — as a click of Select takes it: a guide, a machining of the open part while they
//! are shown, else an occurrence —; a drag erases everything it passes over, at once when it is
//! released (one operation), and Esc drops it. With Shift held, the occurrences taken are
//! hidden instead (guides and machinings are left). What would go is drawn as a ghost.
//! Started on a selection, it erases it at once (the editor, docs/tools/selection.md); selected while it
//! runs, the selection is left alone. Under « Parts », it takes the parts and the guides at any
//! depth.

use glam::DVec2;
use skraft_i18n::t;
use skraft_model::{EntityPath, GuidePath, Id, Operation};

use super::{Command, Cx, Preview, Tool, ToolInput, ToolKind};

/// The distance (pixels) between the points a drag is sampled at, so that a quick stroke
/// misses nothing between two cursor positions.
const SWEEP_STEP_PX: f64 = 4.0;

#[derive(Default)]
pub struct EraseTool {
    hover: Option<Id>,
    /// The part under the cursor at any depth (« Parts »).
    hover_part: Option<EntityPath>,
    /// The drag under way: its last point, and what it passed over, in that order.
    sweep: Option<(DVec2, Vec<Id>)>,
    /// The parts at any depth the drag passed over (« Parts »).
    swept: Vec<EntityPath>,
    /// The guide near the cursor at any depth (« Parts »).
    hover_guide: Option<GuidePath>,
    /// The guides at any depth the drag passed over (« Parts »).
    swept_guides: Vec<GuidePath>,
}

impl EraseTool {
    /// Adds what lies under the segment `from`–`to` to the sweep.
    fn sweep_to(&mut self, to: DVec2, cx: &Cx<'_>) {
        let Some((last, hits)) = &mut self.sweep else { return };
        let from = *last;
        let steps = ((to - from).length() / SWEEP_STEP_PX).ceil().max(1.0) as usize;
        for k in 1..=steps {
            let at = from.lerp(to, k as f64 / steps as f64);
            if cx.parts {
                if let Some(g) = cx.guide_at(at) {
                    if !self.swept_guides.contains(&g) {
                        self.swept_guides.push(g);
                    }
                } else if let Some(p) = cx.part_at(at)
                    && !self.swept.contains(&p)
                {
                    self.swept.push(p);
                }
            } else if let Some(id) = cx.entity_at(at)
                && !hits.contains(&id)
            {
                hits.push(id);
            }
        }
        *last = to;
    }

    /// Erasing the parts `paths` and the guides `guides` at any depth, or with `hide`, hiding
    /// the parts (« Parts »).
    fn apply_parts(paths: &[EntityPath], guides: &[GuidePath], hide: bool, cx: &Cx<'_>) -> Vec<Command> {
        let op = if hide && !paths.is_empty() {
            cx.deep_op(paths, |entities| Operation::SetHidden { context: Id(0), entities, hidden: true })
        } else if !hide && !(paths.is_empty() && guides.is_empty()) {
            cx.erase_deep(paths, guides)
        } else {
            return vec![];
        };
        vec![Command::Apply(op)]
    }

    /// What erasing (or with `hide`, hiding) `ids` takes: hiding takes only occurrences.
    fn taken(ids: &[Id], hide: bool, cx: &Cx<'_>) -> Vec<Id> {
        ids.iter().copied().filter(|id| !hide || cx.is_occurrence(*id)).collect()
    }

    fn apply(ids: &[Id], hide: bool, cx: &Cx<'_>) -> Vec<Command> {
        let entities = Self::taken(ids, hide, cx);
        if entities.is_empty() {
            return vec![];
        }
        let op = if hide { Operation::SetHidden { context: cx.context_def, entities, hidden: true } } else { cx.erase(entities) };
        vec![Command::Apply(op)]
    }
}

impl Tool for EraseTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Erase
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        match input {
            ToolInput::Hover { px } => {
                let free = self.sweep.is_none();
                self.hover = if free && !cx.parts { cx.entity_at(*px) } else { None };
                self.hover_guide = if free && cx.parts { cx.guide_at(*px) } else { None };
                self.hover_part = if free && cx.parts && self.hover_guide.is_none() { cx.part_at(*px) } else { None };
            }
            ToolInput::Click { px, mods } | ToolInput::DoubleClick { px, mods } => {
                self.hover = None;
                self.hover_part = None;
                self.hover_guide = None;
                if cx.parts {
                    if let Some(g) = cx.guide_at(*px) {
                        return Self::apply_parts(&[], &[g], mods.toggle, cx);
                    }
                    return cx.part_at(*px).map_or_else(Vec::new, |p| Self::apply_parts(&[p], &[], mods.toggle, cx));
                }
                return cx.entity_at(*px).map_or_else(Vec::new, |id| Self::apply(&[id], mods.toggle, cx));
            }
            ToolInput::DragStart { px, .. } => {
                self.hover = None;
                self.hover_part = None;
                self.hover_guide = None;
                let guide = if cx.parts { cx.guide_at(*px) } else { None };
                self.swept = if cx.parts && guide.is_none() { cx.part_at(*px).into_iter().collect() } else { Vec::new() };
                self.swept_guides = guide.into_iter().collect();
                self.sweep = Some((*px, if cx.parts { Vec::new() } else { cx.entity_at(*px).into_iter().collect() }));
            }
            ToolInput::Drag { px } => self.sweep_to(*px, cx),
            ToolInput::DragEnd { px, mods } => {
                // Esc during the drag dropped it.
                if self.sweep.is_none() {
                    return vec![];
                }
                self.sweep_to(*px, cx);
                let (_, hits) = self.sweep.take().expect("checked above");
                let swept = std::mem::take(&mut self.swept);
                let swept_guides = std::mem::take(&mut self.swept_guides);
                if cx.parts {
                    return Self::apply_parts(&swept, &swept_guides, mods.toggle, cx);
                }
                return Self::apply(&hits, mods.toggle, cx);
            }
            ToolInput::Key { .. } | ToolInput::Vcb(_) | ToolInput::Value(_) => {}
        }
        vec![]
    }

    fn preview(&self, cx: &Cx<'_>) -> Preview {
        let ids = match &self.sweep {
            Some((_, hits)) => hits.as_slice(),
            None => self.hover.as_slice(),
        };
        let erasing_paths = match &self.sweep {
            Some(_) => self.swept.clone(),
            None => self.hover_part.iter().cloned().collect(),
        };
        // Hiding leaves guides.
        let erasing_guides = match &self.sweep {
            _ if cx.shift_down => Vec::new(),
            Some(_) => self.swept_guides.clone(),
            None => self.hover_guide.iter().cloned().collect(),
        };
        Preview { erasing: Self::taken(ids, cx.shift_down, cx), erasing_paths, erasing_guides, ..Default::default() }
    }

    fn status(&self) -> String {
        if self.sweep.is_some() { t!("tool.erase.hint.sweep") } else { t!("tool.erase.hint.idle") }
    }

    fn busy(&self) -> bool {
        self.sweep.is_some()
    }

    fn leave(&mut self) {
        self.hover = None;
        self.hover_part = None;
        self.hover_guide = None;
    }

    fn picks_parts(&self) -> bool {
        true
    }

    fn reset(&mut self) {
        self.hover = None;
        self.hover_part = None;
        self.sweep = None;
        self.swept.clear();
        self.hover_guide = None;
        self.swept_guides.clear();
    }
}

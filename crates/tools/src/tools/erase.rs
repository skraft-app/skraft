//! Erase (SketchUp's eraser): a click erases the entity of the active context under the
//! cursor — as a click of Select takes it: a guide, a machining of the open part while they
//! are shown, else an occurrence —; a drag erases everything it passes over, at once when it is
//! released (one operation), and Esc drops it. With Shift held, the occurrences taken are
//! hidden instead (guides and machinings are left). What would go is drawn as a ghost. The
//! selection is left alone.

use glam::DVec2;
use solidsketch_i18n::t;
use solidsketch_model::{Id, Operation};

use super::{Command, Cx, Preview, Tool, ToolInput, ToolKind};

/// The distance (pixels) between the points a drag is sampled at, so that a quick stroke
/// misses nothing between two cursor positions.
const SWEEP_STEP_PX: f64 = 4.0;

#[derive(Default)]
pub struct EraseTool {
    hover: Option<Id>,
    /// The drag under way: its last point, and what it passed over, in that order.
    sweep: Option<(DVec2, Vec<Id>)>,
}

impl EraseTool {
    /// Adds what lies under the segment `from`–`to` to the sweep.
    fn sweep_to(&mut self, to: DVec2, cx: &Cx<'_>) {
        let Some((last, hits)) = &mut self.sweep else { return };
        let from = *last;
        let steps = ((to - from).length() / SWEEP_STEP_PX).ceil().max(1.0) as usize;
        for k in 1..=steps {
            if let Some(id) = cx.entity_at(from.lerp(to, k as f64 / steps as f64))
                && !hits.contains(&id)
            {
                hits.push(id);
            }
        }
        *last = to;
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
            ToolInput::Hover { px } => self.hover = if self.sweep.is_some() { None } else { cx.entity_at(*px) },
            ToolInput::Click { px, mods } | ToolInput::DoubleClick { px, mods } => {
                self.hover = None;
                return cx.entity_at(*px).map_or_else(Vec::new, |id| Self::apply(&[id], mods.toggle, cx));
            }
            ToolInput::DragStart { px, .. } => {
                self.hover = None;
                self.sweep = Some((*px, cx.entity_at(*px).into_iter().collect()));
            }
            ToolInput::Drag { px } => self.sweep_to(*px, cx),
            ToolInput::DragEnd { px, mods } => {
                // Esc during the drag dropped it.
                if self.sweep.is_none() {
                    return vec![];
                }
                self.sweep_to(*px, cx);
                let (_, hits) = self.sweep.take().expect("checked above");
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
        Preview { erasing: Self::taken(ids, cx.shift_down, cx), ..Default::default() }
    }

    fn status(&self) -> String {
        if self.sweep.is_some() { t!("tool.erase.hint.sweep") } else { t!("tool.erase.hint.idle") }
    }

    fn busy(&self) -> bool {
        self.sweep.is_some()
    }

    fn leave(&mut self) {
        self.hover = None;
    }

    fn reset(&mut self) {
        self.hover = None;
        self.sweep = None;
    }
}

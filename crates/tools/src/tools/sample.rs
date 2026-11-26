//! « Prélever » (SketchUp's Sample Paint, OpenCutList's Smart Paint `ACTION_PICK`,
//! docs/render/look.md): a click takes the material shown under the cursor — the face's own,
//! else its part's, else none — as the one the paint tool
//! paints with; its tooltip names it. Started on its own, the paint tool then starts with it.
//! Held over the paint tool by the other modifier (the tool stack,
//! docs/tools/interaction.md#tool-stack), each click takes, until the key is released.

use skraft_i18n::t;
use skraft_model::EntityPath;

use super::{Command, Cursor, Cx, DragClicks, Preview, Tool, ToolInput, ToolKind, alt_key};
use crate::scene::Hit;

#[derive(Default)]
pub struct SampleTool {
    hover: Option<Hit>,
    /// Held over another tool, at the last input.
    held: bool,
}

impl SampleTool {
    /// Takes `material` as the paint tool's: on its own, the paint tool starts with it.
    fn take(material: Option<skraft_model::Id>, cx: &Cx<'_>) -> Vec<Command> {
        let mut commands = vec![Command::PaintWith(material)];
        if !cx.held {
            commands.push(Command::Start(ToolKind::Paint));
        }
        commands
    }
}

impl Tool for SampleTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Sample
    }

    fn cursor(&self, _cx: &Cx<'_>) -> Option<Cursor> {
        Some(Cursor::Picker)
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        self.held = cx.held;
        match input {
            ToolInput::Hover { px } => self.hover = cx.pick(*px),
            ToolInput::Click { px, .. } => {
                let Some(hit) = cx.pick(*px) else { return vec![] };
                return Self::take(cx.material_shown(&hit.path, hit.face), cx);
            }
            _ => {}
        }
        vec![]
    }

    /// The material it would take, by the cursor.
    fn preview(&self, cx: &Cx<'_>) -> Preview {
        let Some(hit) = &self.hover else { return Preview::default() };
        let name = cx.material_shown(&hit.path, hit.face).and_then(|m| cx.doc.material(m).ok()).map(|m| m.name.clone());
        Preview { tip: Some(name.unwrap_or_else(|| t!("panel.materials.none"))), ..Default::default() }
    }

    fn status(&self) -> String {
        if self.held { t!("tool.sample.hint_held", alt_key = alt_key()) } else { t!("tool.sample.hint") }
    }

    /// A single click: never between steps.
    fn busy(&self) -> bool {
        false
    }

    /// A drag takes where it is released.
    fn drag_clicks(&self) -> DragClicks {
        DragClicks::Release
    }

    /// The level chosen in the tree drop-up: a part's material taken (an assembly has none).
    fn tree_pick(&self, path: &EntityPath, cx: &Cx<'_>) -> Option<Vec<Command>> {
        Some(Self::take(cx.part_material(path), cx))
    }

    fn leave(&mut self) {
        self.hover = None;
    }

    fn reset(&mut self) {
        self.hover = None;
    }
}

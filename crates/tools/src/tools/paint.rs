//! Paint (SketchUp's paint bucket, §7 quater): a click paints the occurrence under the
//! cursor — the whole selection when it is in it — or a single face of its part, with the
//! current material (none: the paint is removed). The option says which.

use solidsketch_i18n::t;
use solidsketch_model::{EntityPath, Operation};

use super::{Command, Cx, OptionChoice, OptionValue, Preview, Tool, ToolInput, ToolKind, ToolOption};
use crate::scene::Hit;

/// The target option's key and values.
pub const TARGET: &str = "target";
const OCCURRENCE: &str = "occurrence";
const FACE: &str = "face";

fn faces(cx: &Cx<'_>) -> bool {
    cx.choice(TARGET) == Some(FACE)
}

#[derive(Default)]
pub struct PaintTool {
    hover: Option<Hit>,
}

impl PaintTool {
    fn paint(&self, hit: &Hit, cx: &Cx<'_>) -> Result<Operation, String> {
        if faces(cx) {
            let (part, _, _) = cx.doc.resolve_part(&hit.path).map_err(|e| e.to_string())?;
            return Ok(Operation::PaintFaces { part, faces: vec![hit.face], material: cx.material });
        }
        let id = cx.selectable(&hit.path).ok_or_else(|| t!("tool.paint.outside"))?;
        let entities = if cx.selection.contains(&id) { cx.selection.to_vec() } else { vec![id] };
        Ok(Operation::PaintOccurrences { context: cx.context_def, entities, material: cx.material })
    }
}

impl Tool for PaintTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Paint
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        match input {
            ToolInput::Hover { px } => self.hover = cx.pick(*px),
            ToolInput::Click { px, .. } => {
                let Some(hit) = cx.pick(*px) else { return vec![] };
                return match self.paint(&hit, cx) {
                    Ok(op) => vec![Command::Apply(op)],
                    Err(why) => vec![Command::Say(why)],
                };
            }
            _ => {}
        }
        vec![]
    }

    fn preview(&self, cx: &Cx<'_>) -> Preview {
        let Some(hit) = &self.hover else { return Preview::default() };
        if faces(cx) {
            // A face belongs to the part: every occurrence of it changes.
            let path: EntityPath = hit.path.clone();
            return Preview { hover_solid: Some(path.clone()), propagate: Some(path), ..Default::default() };
        }
        Preview { hover: cx.selectable(&hit.path), ..Default::default() }
    }

    fn options(&self, cx: &Cx<'_>) -> Vec<ToolOption> {
        let value = if faces(cx) { FACE } else { OCCURRENCE };
        vec![ToolOption {
            key: TARGET,
            label: t!("tool.paint.option.target"),
            tip: t!("tool.paint.option.target_tip"),
            value: OptionValue::Choice(value.into()),
            choices: vec![
                OptionChoice { value: OCCURRENCE, label: t!("tool.paint.option.occurrence"), tip: t!("tool.paint.option.occurrence_tip"), icon: None },
                OptionChoice { value: FACE, label: t!("tool.paint.option.face"), tip: t!("tool.paint.option.face_tip"), icon: None },
            ],
            saved: true,
            icon: None,
        }]
    }

    fn status(&self) -> String {
        t!("tool.paint.hint")
    }

    fn busy(&self) -> bool {
        false
    }

    fn reset(&mut self) {}
}

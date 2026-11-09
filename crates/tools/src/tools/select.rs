//! Select: click, Shift+click, double-click to enter a group or component, double-click in the
//! void (or outside the context) to leave it.

use glam::DVec2;
use solidsketch_model::Id;

use super::{Command, Cx, Preview, Tool, ToolInput, ToolKind};

#[derive(Default)]
pub struct SelectTool {
    hover: Option<Id>,
}

impl Tool for SelectTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Select
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        let target = |px: DVec2| cx.pick(px).map(|h| cx.selectable(&h.path));
        match input {
            ToolInput::Hover { px } => {
                self.hover = target(*px).flatten();
                vec![]
            }
            ToolInput::Click { px, mods } => match target(*px) {
                Some(Some(id)) if mods.toggle => {
                    let mut sel = cx.selection.to_vec();
                    match sel.iter().position(|s| *s == id) {
                        Some(i) => {
                            sel.remove(i);
                        }
                        None => sel.push(id),
                    }
                    vec![Command::Select(sel)]
                }
                Some(Some(id)) => vec![Command::Select(vec![id])],
                // Outside the context, or the void.
                _ if mods.toggle => vec![],
                _ => vec![Command::Select(vec![])],
            },
            ToolInput::DoubleClick { px, .. } => match target(*px) {
                Some(Some(id)) => {
                    let is_instance =
                        cx.doc.definition(cx.context_def).ok().and_then(|d| d.entity(id)).and_then(|e| e.as_instance()).is_some();
                    if is_instance { vec![Command::Enter(id)] } else { vec![] }
                }
                _ if !cx.context.is_empty() => vec![Command::Exit],
                _ => vec![],
            },
            ToolInput::Key { .. } | ToolInput::Vcb(_) => vec![],
        }
    }

    fn preview(&self, _: &Cx<'_>) -> Preview {
        Preview { hover: self.hover, ..Default::default() }
    }

    fn status(&self) -> String {
        "Cliquer pour sélectionner · Maj : ajouter/retirer · Double-clic : entrer dans un groupe, ou en sortir".into()
    }

    fn busy(&self) -> bool {
        false
    }

    fn reset(&mut self) {
        self.hover = None;
    }
}

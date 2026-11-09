//! Union / Subtract / Intersect: click the target solid, then the tool solid, in any contexts
//! (ARCHITECTURE.md §4: the target is changed in its own context, the tool is consumed).

use solidsketch_model::{Boolean, EntityPath, Operation};

use super::{Command, Cx, Preview, Tool, ToolInput, ToolKind};

pub struct BooleanTool {
    op: Boolean,
    target: Option<EntityPath>,
    hover: Option<EntityPath>,
}

impl BooleanTool {
    pub fn new(op: Boolean) -> Self {
        BooleanTool { op, target: None, hover: None }
    }

    fn noun(&self) -> &'static str {
        match self.op {
            Boolean::Union => "unir",
            Boolean::Difference => "soustraire",
            Boolean::Intersection => "intersecter",
        }
    }
}

impl Tool for BooleanTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Boolean(self.op)
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        match input {
            ToolInput::Hover { px } => self.hover = cx.pick(*px).map(|h| h.path),
            ToolInput::Click { px, .. } => {
                let Some(path) = cx.pick(*px).map(|h| h.path) else { return vec![] };
                match self.target.take() {
                    None => self.target = Some(path),
                    Some(target) if target == path => {}
                    Some(target) => {
                        return vec![Command::Apply(Operation::Boolean { target, tool: path, op: self.op, keep_tool: false })];
                    }
                }
            }
            _ => {}
        }
        vec![]
    }

    fn preview(&self, _: &Cx<'_>) -> Preview {
        Preview { hover_solid: self.hover.clone(), picked_solid: self.target.clone(), ..Default::default() }
    }

    fn status(&self) -> String {
        match self.target {
            None => format!("Cliquer le solide à modifier ({})", self.noun()),
            Some(_) => format!("Cliquer le solide outil (il sera consommé) pour {}", self.noun()),
        }
    }

    fn busy(&self) -> bool {
        self.target.is_some()
    }

    fn reset(&mut self) {
        self.target = None;
    }
}

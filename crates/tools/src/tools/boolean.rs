//! Union / Subtract / Intersect: click the target part, then the tool part, in any contexts
//! (docs/model/kernel.md: the target is changed in its own axes, the tool is consumed). The
//! scope option says whether every occurrence of the target's part changes, or only the one
//! clicked (docs/model/data-model.md).

use super::DragClicks;
use skraft_i18n::t;
use skraft_model::{Boolean, EntityPath, Operation, Scope};

use super::{Command, Cx, OptionChoice, OptionValue, Preview, Tool, ToolInput, ToolKind, ToolOption};

/// The scope option's key and values.
pub const SCOPE: &str = "scope";
const ALL: &str = "all";
const THIS: &str = "this";

fn scope(cx: &Cx<'_>) -> Scope {
    if cx.choice(SCOPE) == Some(THIS) { Scope::This } else { Scope::All }
}

pub struct BooleanTool {
    op: Boolean,
    target: Option<EntityPath>,
    hover: Option<EntityPath>,
}

impl BooleanTool {
    pub fn new(op: Boolean) -> Self {
        BooleanTool { op, target: None, hover: None }
    }

    /// What the tool does, inside its hints.
    fn verb(&self) -> String {
        match self.op {
            Boolean::Union => t!("tool.boolean.union.verb"),
            Boolean::Difference => t!("tool.boolean.difference.verb"),
            Boolean::Intersection => t!("tool.boolean.intersection.verb"),
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
                        let op = Operation::Boolean { target, tool: path, op: self.op, scope: scope(cx) };
                        return vec![Command::Apply(op)];
                    }
                }
            }
            _ => {}
        }
        vec![]
    }

    fn preview(&self, cx: &Cx<'_>) -> Preview {
        // What a click would change: the target picked, else the part under the cursor.
        let aimed = self.target.clone().or_else(|| self.hover.clone());
        let propagate = aimed.filter(|_| scope(cx) == Scope::All);
        Preview { hover_solid: self.hover.clone(), picked_solid: self.target.clone(), propagate, ..Default::default() }
    }

    fn options(&self, cx: &Cx<'_>) -> Vec<ToolOption> {
        let aimed = self.target.as_ref().or(self.hover.as_ref());
        // No scope for a part placed once: both choices are the same (docs/model/data-model.md).
        let all = match aimed.map(|p| cx.copies_shown(p)) {
            Some(n) if n > 1 => t!("tool.boolean.option.all_count", count = n),
            Some(_) => return Vec::new(),
            None => t!("tool.boolean.option.all"),
        };
        let value = if scope(cx) == Scope::This { THIS } else { ALL };
        vec![ToolOption {
            key: SCOPE,
            label: t!("tool.boolean.option.scope"),
            tip: t!("tool.boolean.option.scope_tip"),
            value: OptionValue::Choice(value.into()),
            choices: vec![
                OptionChoice { value: ALL, label: all, tip: t!("tool.boolean.option.all_tip"), icon: Some("scope_all") },
                OptionChoice {
                    value: THIS,
                    label: t!("tool.boolean.option.this"),
                    tip: t!("tool.boolean.option.this_tip"),
                    icon: Some("scope_this"),
                },
            ],
            saved: true,
            icon: None,
            swatch: None,
        }]
    }

    fn status(&self) -> String {
        match self.target {
            None => t!("tool.boolean.hint.target", verb = self.verb()),
            Some(_) => t!("tool.boolean.hint.tool", verb = self.verb()),
        }
    }

    /// A drag from the target to the tool.
    fn drag_clicks(&self) -> DragClicks {
        if self.target.is_none() { DragClicks::PressAndRelease } else { DragClicks::Release }
    }

    fn busy(&self) -> bool {
        self.target.is_some()
    }

    /// Both clicks pick a part: the target, then the tool.
    fn picking(&self) -> bool {
        true
    }

    fn reset(&mut self) {
        self.target = None;
    }
}

//! Union / Subtract / Intersect, after OpenCutList's solid tools (`SmartReshapeSolidActionHandler`):
//! the parts A (the targets), then the parts B (the tools), in any contexts
//! (docs/tools/solid-tools.md). A click takes one part and goes on; Shift gathers several, Enter
//! goes on (A, then B, the operation applied). A union takes one target. The scope option says
//! whether every occurrence of a target's part changes, or only the one clicked
//! (docs/model/data-model.md); « Conserver A », « Conserver B » and the broom keep the targets,
//! keep the tools, drop the tools' parts left unused.

use super::DragClicks;
use skraft_i18n::t;
use skraft_model::{Boolean, EntityPath, Operation, Scope, Sign};

use super::parts_picked::PartsPicked;
use super::{Command, Cx, Key, OptionChoice, OptionValue, Preview, Steps, Tool, ToolInput, ToolKind, ToolOption};

/// The scope option's key and values.
pub const SCOPE: &str = "scope";
const ALL: &str = "all";
const THIS: &str = "this";
/// The targets kept, the result in a new part beside each (OCL's `keep_a`).
pub const KEEP_TARGETS: &str = "keep_targets";
/// The tools kept (OCL's `keep_b`).
pub const KEEP_TOOLS: &str = "keep_tools";
/// The tools' parts left without occurrence dropped, even named (OCL's
/// `remove_unused_definitions`).
pub const DROP_UNUSED: &str = "drop_unused";

/// The three solid tools, which share their options but the scope (OCL's
/// `get_action_option_sync_actions`).
pub const SOLID_TOOLS: &[ToolKind] =
    &[ToolKind::Boolean(Boolean::Union), ToolKind::Boolean(Boolean::Difference), ToolKind::Boolean(Boolean::Intersection)];

fn scope(cx: &Cx<'_>) -> Scope {
    if cx.choice(SCOPE) == Some(THIS) { Scope::This } else { Scope::All }
}

pub struct BooleanTool {
    op: Boolean,
    /// The targets (A).
    a: PartsPicked,
    /// The tools (B), once the targets are taken.
    b: Option<PartsPicked>,
    hover: Option<EntityPath>,
}

impl BooleanTool {
    pub fn new(op: Boolean) -> Self {
        BooleanTool { op, a: PartsPicked::default(), b: None, hover: None }
    }

    /// What the tool does, inside its hints.
    fn verb(&self) -> String {
        match self.op {
            Boolean::Union => t!("tool.boolean.union.verb"),
            Boolean::Difference => t!("tool.boolean.difference.verb"),
            Boolean::Intersection => t!("tool.boolean.intersection.verb"),
        }
    }

    /// Does the step now take several parts? A union takes one target.
    fn gathers(&self) -> bool {
        self.b.is_some() || self.op != Boolean::Union
    }

    /// The set the step now picks into.
    fn set(&self) -> &PartsPicked {
        self.b.as_ref().unwrap_or(&self.a)
    }

    /// The operation on what is picked, the tool back at its first step.
    fn operate(&mut self, cx: &Cx<'_>) -> Vec<Command> {
        let tools = self.b.take().map(|b| b.paths).unwrap_or_default();
        let targets = std::mem::take(&mut self.a).paths;
        if targets.is_empty() || tools.is_empty() {
            return vec![];
        }
        vec![Command::Apply(Operation::Boolean {
            targets,
            tools,
            op: self.op,
            scope: scope(cx),
            keep_targets: cx.toggle(KEEP_TARGETS),
            keep_tools: cx.toggle(KEEP_TOOLS),
            drop_unused: cx.toggle(DROP_UNUSED),
        })]
    }

    /// Enter, or a click that ends a set: on to the tools, or the operation.
    fn validate(&mut self, cx: &Cx<'_>) -> Vec<Command> {
        match &self.b {
            None if !self.a.paths.is_empty() => {
                self.b = Some(PartsPicked::default());
                vec![]
            }
            Some(b) if !b.paths.is_empty() => self.operate(cx),
            _ => vec![],
        }
    }
}

impl Tool for BooleanTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Boolean(self.op)
    }

    /// The targets (OCL's `STATE_SELECT_SRC`), then the tools (`STATE_SELECT_CUT`).
    fn cursor(&self, _cx: &Cx<'_>) -> Option<crate::tools::Cursor> {
        let first = self.b.is_none();
        Some(match self.op {
            Boolean::Union if first => crate::tools::Cursor::SelectUniteA,
            Boolean::Union => crate::tools::Cursor::SelectUniteB,
            Boolean::Difference if first => crate::tools::Cursor::SelectSubtractA,
            Boolean::Difference => crate::tools::Cursor::SelectSubtractB,
            Boolean::Intersection if first => crate::tools::Cursor::SelectIntersectA,
            Boolean::Intersection => crate::tools::Cursor::SelectIntersectB,
        })
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        match input {
            ToolInput::Hover { px } => {
                // A target is not a tool too (OCL's `_can_activate_part?`).
                self.hover = cx.pick(*px).map(|h| h.path).filter(|p| self.b.is_none() || !self.a.paths.contains(p));
            }
            ToolInput::Click { px, mods } => {
                let Some(hit) = cx.pick(*px) else { return vec![] };
                let path = hit.path;
                let gathers = self.gathers();
                let set = match &mut self.b {
                    Some(_) if self.a.paths.contains(&path) => return vec![],
                    Some(b) => b,
                    None => &mut self.a,
                };
                if !gathers {
                    set.clear();
                }
                set.toggle(path, hit.point);
                if set.paths.is_empty() {
                    return vec![];
                }
                // Shift held, or several already: the set waits for Enter.
                if gathers && (mods.toggle || set.gathering) {
                    set.gathering = true;
                    return vec![];
                }
                return self.validate(cx);
            }
            ToolInput::Key { key: Key::Enter, .. } => return self.validate(cx),
            // Shift let go with a set gathering: Enter goes on (OCL's notifications).
            ToolInput::Key { key: Key::Shift, .. } if self.gathers() && !self.set().paths.is_empty() => {
                let say = if self.b.is_none() { t!("tool.boolean.say.validate_a") } else { t!("tool.boolean.say.validate_b") };
                return vec![Command::Say(say)];
            }
            _ => {}
        }
        vec![]
    }

    fn preview(&self, cx: &Cx<'_>) -> Preview {
        // What a click would change: the first target, else the part under the cursor.
        let aimed = self.a.paths.first().cloned().or_else(|| self.hover.clone().filter(|_| self.b.is_none()));
        let propagate = aimed.filter(|_| scope(cx) == Scope::All);
        Preview {
            hover_solid: self.hover.clone(),
            picked_solids: self.a.paths.clone(),
            picked_tools: self.b.as_ref().map(|b| b.paths.clone()).unwrap_or_default(),
            // The tools as the bodies they become in their targets (docs/model/bodies.md).
            picked_sign: Some(match self.op {
                Boolean::Union => Sign::Positive,
                Boolean::Difference => Sign::Negative,
                Boolean::Intersection => Sign::Intersect,
            }),
            badges: self
                .a
                .badges(t!("tool.boolean.step.a"))
                .chain(self.b.iter().flat_map(|b| b.badges(t!("tool.boolean.step.b"))))
                .collect(),
            propagate,
            ..Default::default()
        }
    }

    fn options(&self, cx: &Cx<'_>) -> Vec<ToolOption> {
        let mut out = Vec::new();
        let aimed = self.a.paths.first().or(self.hover.as_ref().filter(|_| self.b.is_none()));
        // No scope for a part placed once: both choices are the same (docs/model/data-model.md).
        let all = match aimed.map(|p| cx.copies_shown(p)) {
            Some(n) if n > 1 => Some(t!("tool.boolean.option.all_count", count = n)),
            Some(_) => None,
            None => Some(t!("tool.boolean.option.all")),
        };
        if let Some(all) = all {
            let value = if scope(cx) == Scope::This { THIS } else { ALL };
            out.push(ToolOption {
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
            });
        }
        let toggle = |key, label, tip, icon| ToolOption {
            key,
            label,
            tip,
            value: OptionValue::Toggle(cx.toggle(key)),
            choices: Vec::new(),
            saved: true,
            icon,
            swatch: None,
        };
        out.push(toggle(KEEP_TARGETS, t!("tool.boolean.option.keep_targets"), t!("tool.boolean.option.keep_targets_tip"), None));
        out.push(toggle(KEEP_TOOLS, t!("tool.boolean.option.keep_tools"), t!("tool.boolean.option.keep_tools_tip"), None));
        out.push(toggle(
            DROP_UNUSED,
            t!("tool.boolean.option.drop_unused"),
            t!("tool.boolean.option.drop_unused_tip"),
            Some("drop_unused"),
        ));
        out
    }

    fn status(&self) -> String {
        let verb = self.verb();
        match &self.b {
            None if self.op == Boolean::Union => t!("tool.boolean.hint.target", verb = verb),
            None if self.a.gathering => t!("tool.boolean.hint.targets_gather"),
            None => t!("tool.boolean.hint.targets", verb = verb),
            Some(b) if b.gathering => t!("tool.boolean.hint.tools_gather", verb = verb),
            Some(_) => t!("tool.boolean.hint.tools", verb = verb),
        }
    }

    /// A then B (OCL's steps bar of the solid actions).
    fn steps(&self) -> Option<Steps> {
        let text = match &self.b {
            None if self.op == Boolean::Union => t!("tool.boolean.step.target"),
            None => t!("tool.boolean.step.targets"),
            Some(_) => t!("tool.boolean.step.tools"),
        };
        // Named as the texts and options call them (« Conserver A »).
        let names = vec![t!("tool.boolean.step.a"), t!("tool.boolean.step.b")];
        Some(Steps { count: 2, index: usize::from(self.b.is_some()), text, names })
    }

    /// A set gathering several parts waits for Enter (OCL's `need_validation?`).
    fn awaits_validation(&self) -> bool {
        let set = self.set();
        self.gathers() && set.gathering && !set.paths.is_empty()
    }

    /// A drag from a target to a tool.
    fn drag_clicks(&self) -> DragClicks {
        if self.b.is_none() && self.a.paths.is_empty() { DragClicks::PressAndRelease } else { DragClicks::Release }
    }

    fn busy(&self) -> bool {
        self.b.is_some() || !self.a.paths.is_empty()
    }

    /// Both steps pick parts: the targets, then the tools.
    fn picking(&self) -> bool {
        true
    }

    /// Esc: the tools picked let go, then back to the targets (none kept, as OCL's).
    fn back(&mut self) {
        match &mut self.b {
            Some(b) if !b.paths.is_empty() => b.clear(),
            Some(_) => self.reset(),
            None => self.a.clear(),
        }
    }

    fn reset(&mut self) {
        self.a.clear();
        self.b = None;
    }
}

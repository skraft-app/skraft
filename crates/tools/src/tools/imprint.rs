//! « Empreindre » (docs/decisions/0055-linked-bodies-through-imprints.md): a subtraction that
//! keeps its tools, linked. It picks as « Soustraire » (docs/tools/solid-tools.md): A the parts
//! gone through, then B the sources that go into them. A click takes one part and goes on; Shift
//! gathers several, Enter goes on; the sources picked, the kernel is asked. Each source is tried
//! against each part A, and the copies placed as the parts pointed come along; the confirmation
//! says how many. The selection is ignored: the tool always starts at A.

use skraft_i18n::t;
use skraft_model::{EntityPath, Id, Sign};

use super::parts_picked::PartsPicked;
use super::{Command, Cursor, Cx, DragClicks, Key, Preview, Steps, Tool, ToolInput, ToolKind};

/// The occurrences a part's path goes through, from the model, its own last.
pub fn occurrences(path: &EntityPath) -> Vec<Id> {
    let mut out = path.instances.clone();
    out.push(path.entity);
    out
}

#[derive(Default)]
pub struct ImprintTool {
    /// The parts gone through (A).
    receivers: PartsPicked,
    /// The sources (B), once the parts gone through are taken.
    sources: Option<PartsPicked>,
    hover: Option<EntityPath>,
}

impl ImprintTool {
    /// Can `path` be taken as a source: no part A, and one of them within its reach.
    fn reaches(&self, cx: &Cx<'_>, path: &EntityPath) -> bool {
        let from = occurrences(path);
        !self.receivers.paths.contains(path) && self.receivers.paths.iter().any(|r| cx.doc.may_imprint(&from, &occurrences(r)))
    }

    /// The set the step now picks into.
    fn set(&self) -> &PartsPicked {
        self.sources.as_ref().unwrap_or(&self.receivers)
    }

    /// Enter, or a click that ends a set: on to the sources, or the kernel asked, the tool back at
    /// its first step (its marks drawn by the editor until the confirmation closes).
    fn validate(&mut self) -> Vec<Command> {
        let shown = Box::new(self.marks());
        match &self.sources {
            None if !self.receivers.paths.is_empty() => {
                self.sources = Some(PartsPicked::default());
                vec![]
            }
            Some(s) if !s.paths.is_empty() => {
                let command = Command::Imprint {
                    sources: s.paths.iter().map(occurrences).collect(),
                    receivers: self.receivers.paths.iter().map(occurrences).collect(),
                    shown,
                };
                self.reset();
                vec![command]
            }
            _ => vec![],
        }
    }

    /// As « Soustraire »'s marks: the parts gone through as targets, the sources as the
    /// machinings they become, each with its step's badge.
    fn marks(&self) -> Preview {
        let a = self.receivers.badges(t!("steps.a"));
        Preview {
            picked_solids: self.receivers.paths.clone(),
            picked_tools: self.sources.as_ref().map(|s| s.paths.clone()).unwrap_or_default(),
            picked_sign: Some(Sign::Negative),
            badges: a.chain(self.sources.iter().flat_map(|s| s.badges(t!("steps.b")))).collect(),
            ..Default::default()
        }
    }

    /// A part placed, by a path the tool picks: an occurrence of the active context, or one
    /// deeper under « Parts ».
    fn part_path(cx: &Cx<'_>, path: EntityPath) -> Option<EntityPath> {
        let o = cx.doc.resolve(&path).ok()?;
        cx.doc.part(o.component).is_ok().then_some(path)
    }
}

impl Tool for ImprintTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Imprint
    }

    fn cursor(&self, _cx: &Cx<'_>) -> Option<Cursor> {
        Some(if self.set().gathering { Cursor::SelectPlusMinus } else { Cursor::SelectPart })
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        match input {
            ToolInput::Hover { px } => {
                let path = cx.pick(*px).and_then(|h| Self::part_path(cx, h.path));
                self.hover = match &self.sources {
                    None => path,
                    Some(_) => path.filter(|p| self.reaches(cx, p)),
                };
            }
            ToolInput::Click { px, mods } => {
                let Some((path, point)) = cx.pick(*px).and_then(|h| Some((Self::part_path(cx, h.path)?, h.point))) else { return vec![] };
                if self.sources.is_some() {
                    if self.receivers.paths.contains(&path) {
                        return vec![];
                    }
                    if !self.reaches(cx, &path) {
                        return vec![Command::Say(t!("tool.imprint.out_of_reach"))];
                    }
                }
                let set = self.sources.as_mut().unwrap_or(&mut self.receivers);
                set.toggle(path, point);
                if set.paths.is_empty() {
                    return vec![];
                }
                // Shift held, or several already: the set waits for Enter.
                if mods.toggle || set.gathering {
                    set.gathering = true;
                    return vec![];
                }
                return self.validate();
            }
            ToolInput::Key { key: Key::Enter, .. } => return self.validate(),
            // Shift let go with a set gathering: Enter goes on.
            ToolInput::Key { key: Key::Shift, .. } if self.set().gathering && !self.set().paths.is_empty() => {
                let say = if self.sources.is_none() { t!("tool.imprint.say.validate_a") } else { t!("tool.imprint.say.validate_b") };
                return vec![Command::Say(say)];
            }
            _ => {}
        }
        vec![]
    }

    fn preview(&self, _cx: &Cx<'_>) -> Preview {
        Preview { hover_solid: self.hover.clone(), ..self.marks() }
    }

    fn status(&self) -> String {
        match &self.sources {
            None if self.receivers.gathering => t!("tool.imprint.hint.receivers_gather"),
            None => t!("tool.imprint.hint.receivers"),
            Some(s) if s.gathering => t!("tool.imprint.hint.sources_gather"),
            Some(_) => t!("tool.imprint.hint.sources"),
        }
    }

    /// A the parts gone through, then B the sources.
    fn steps(&self) -> Option<Steps> {
        let text = if self.sources.is_none() { t!("tool.imprint.step.receivers") } else { t!("tool.imprint.step.sources") };
        let names = vec![t!("steps.a"), t!("steps.b")];
        Some(Steps { count: 2, index: usize::from(self.sources.is_some()), text, names })
    }

    fn awaits_validation(&self) -> bool {
        let set = self.set();
        set.gathering && !set.paths.is_empty()
    }

    /// A drag from a part gone through to a source.
    fn drag_clicks(&self) -> DragClicks {
        if self.sources.is_none() && self.receivers.paths.is_empty() { DragClicks::PressAndRelease } else { DragClicks::Release }
    }

    fn busy(&self) -> bool {
        self.sources.is_some() || !self.receivers.paths.is_empty()
    }

    fn picking(&self) -> bool {
        true
    }

    /// Esc: the sources let go, then back to A, none kept.
    fn back(&mut self) {
        match &mut self.sources {
            Some(s) if !s.paths.is_empty() => s.clear(),
            _ => self.reset(),
        }
    }

    fn reset(&mut self) {
        *self = ImprintTool::default();
    }
}

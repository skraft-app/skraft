//! « Empreindre » (docs/decisions/0055-linked-bodies-through-imprints.md): the parts that imprint
//! their shape, then the parts they go into, as the solid tools pick theirs
//! (docs/tools/solid-tools.md). A click takes one part and goes on; Shift gathers several, Enter
//! goes on; the destinations picked, the kernel is asked. Each source is tried against each
//! destination, and the copies placed as the parts pointed come along; the confirmation says how
//! many. Started with parts selected, they are the sources and it waits for the destinations.

use skraft_i18n::t;
use skraft_model::{EntityPath, Id};

use super::{Command, Cursor, Cx, DragClicks, Key, Preview, Steps, Tool, ToolInput, ToolKind};

/// The occurrences a part's path goes through, from the model, its own last.
pub fn occurrences(path: &EntityPath) -> Vec<Id> {
    let mut out = path.instances.clone();
    out.push(path.entity);
    out
}

/// A set of parts the tool picks: once it holds more than one, or Shift added to it, it waits
/// for Enter.
#[derive(Default)]
struct Picked {
    paths: Vec<EntityPath>,
    gathering: bool,
}

impl Picked {
    fn toggle(&mut self, path: EntityPath) {
        if let Some(i) = self.paths.iter().position(|p| *p == path) {
            self.paths.remove(i);
        } else {
            self.paths.push(path);
            self.gathering |= self.paths.len() > 1;
        }
    }
}

#[derive(Default)]
pub struct ImprintTool {
    sources: Picked,
    /// The destinations, once the sources are taken.
    receivers: Option<Picked>,
    hover: Option<EntityPath>,
}

impl ImprintTool {
    /// Can `path` be taken as a destination: no source, and within the reach of one.
    fn reached(&self, cx: &Cx<'_>, path: &EntityPath) -> bool {
        let to = occurrences(path);
        !self.sources.paths.contains(path) && self.sources.paths.iter().any(|s| cx.doc.may_imprint(&occurrences(s), &to))
    }

    /// The set the step now picks into.
    fn set(&self) -> &Picked {
        self.receivers.as_ref().unwrap_or(&self.sources)
    }

    /// Enter, or a click that ends a set: on to the destinations, or the kernel asked, the tool
    /// back at its first step.
    fn validate(&mut self) -> Vec<Command> {
        match &self.receivers {
            None if !self.sources.paths.is_empty() => {
                self.receivers = Some(Picked::default());
                vec![]
            }
            Some(r) if !r.paths.is_empty() => {
                let command = Command::Imprint {
                    sources: self.sources.paths.iter().map(occurrences).collect(),
                    receivers: r.paths.iter().map(occurrences).collect(),
                };
                self.reset();
                vec![command]
            }
            _ => vec![],
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

    /// The parts selected, if any, are the sources: it waits for the destinations.
    fn started(&mut self, cx: &Cx<'_>) {
        let picked = cx.deep.iter().cloned().chain(cx.selection.iter().map(|id| EntityPath {
            instances: cx.context.to_vec(),
            entity: *id,
            group: Vec::new(),
        }));
        let paths: Vec<EntityPath> = picked.filter_map(|p| Self::part_path(cx, p)).collect();
        if !paths.is_empty() {
            self.sources = Picked { gathering: paths.len() > 1, paths };
            self.receivers = Some(Picked::default());
        }
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        match input {
            ToolInput::Hover { px } => {
                let path = cx.pick(*px).and_then(|h| Self::part_path(cx, h.path));
                self.hover = match &self.receivers {
                    None => path,
                    Some(_) => path.filter(|p| self.reached(cx, p)),
                };
            }
            ToolInput::Click { px, mods } => {
                let Some(path) = cx.pick(*px).and_then(|h| Self::part_path(cx, h.path)) else { return vec![] };
                if self.receivers.is_some() {
                    if self.sources.paths.contains(&path) {
                        return vec![];
                    }
                    if !self.reached(cx, &path) {
                        return vec![Command::Say(t!("tool.imprint.out_of_reach"))];
                    }
                }
                let set = self.receivers.as_mut().unwrap_or(&mut self.sources);
                set.toggle(path);
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
                let say = if self.receivers.is_none() { t!("tool.imprint.say.validate_sources") } else { t!("tool.imprint.say.validate") };
                return vec![Command::Say(say)];
            }
            _ => {}
        }
        vec![]
    }

    fn preview(&self, _cx: &Cx<'_>) -> Preview {
        Preview {
            hover_solid: self.hover.clone(),
            picked_solids: self.sources.paths.clone(),
            picked_tools: self.receivers.as_ref().map(|r| r.paths.clone()).unwrap_or_default(),
            ..Default::default()
        }
    }

    fn status(&self) -> String {
        match &self.receivers {
            None if self.sources.gathering => t!("tool.imprint.hint.sources_gather"),
            None => t!("tool.imprint.hint.sources"),
            Some(r) if r.gathering => t!("tool.imprint.hint.receivers_gather"),
            Some(_) => t!("tool.imprint.hint.receivers"),
        }
    }

    /// The sources, then where they go.
    fn steps(&self) -> Option<Steps> {
        let text = if self.receivers.is_none() { t!("tool.imprint.step.sources") } else { t!("tool.imprint.step.receivers") };
        Some(Steps { count: 2, index: usize::from(self.receivers.is_some()), text })
    }

    fn awaits_validation(&self) -> bool {
        let set = self.set();
        set.gathering && !set.paths.is_empty()
    }

    /// A drag from a source to a destination.
    fn drag_clicks(&self) -> DragClicks {
        if self.receivers.is_none() && self.sources.paths.is_empty() { DragClicks::PressAndRelease } else { DragClicks::Release }
    }

    fn busy(&self) -> bool {
        self.receivers.is_some() || !self.sources.paths.is_empty()
    }

    fn picking(&self) -> bool {
        true
    }

    /// Esc: the destinations let go, then back to the sources, none kept.
    fn back(&mut self) {
        match &mut self.receivers {
            Some(r) if !r.paths.is_empty() => *r = Picked::default(),
            _ => self.reset(),
        }
    }

    fn reset(&mut self) {
        *self = ImprintTool::default();
    }
}

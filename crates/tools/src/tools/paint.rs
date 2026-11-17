//! Paint (SketchUp's paint bucket, §7 quater): a click paints the occurrence under the
//! cursor — the whole selection when it is in it — or a single face of its part, with the
//! current material (none: the paint is removed). The option says which. Under « Parts »
//! (§6 septendecies), the part under the cursor at any depth, in its assembly — the parts selected
//! when it is one of them —, and the level chosen in the tree drop-up. Painting occurrences, a
//! drag draws a rectangle: what it encloses (left to right) or touches (right to left) is
//! painted, at once when it is released (one step); Esc drops it.

use super::DragClicks;
use solidsketch_i18n::t;
use solidsketch_model::{EntityPath, Id, Operation};

use super::select::{marquee_hits, marquee_parts};
use super::{Command, Cx, Marquee, OptionChoice, OptionValue, Preview, Tool, ToolInput, ToolKind, ToolOption};
use crate::scene::Hit;

/// The target option's key and values.
pub const TARGET: &str = "target";
const OCCURRENCE: &str = "occurrence";
const FACE: &str = "face";

fn faces(cx: &Cx<'_>) -> bool {
    cx.choice(TARGET) == Some(FACE)
}

/// A drag shorter than this (pixels, both ways) is a click.
const MIN_MARQUEE: f64 = 3.0;

#[derive(Default)]
pub struct PaintTool {
    hover: Option<Hit>,
    /// The rectangle dragged (painting occurrences).
    marquee: Option<Marquee>,
    /// Painting faces, as the option said at the last input.
    faces: bool,
}

impl PaintTool {
    /// The part `hit` is on, under the active context (« Parts »).
    fn part(hit: &Hit, cx: &Cx<'_>) -> Option<EntityPath> {
        Some(hit.path.part_path()).filter(|p| p.instances.starts_with(cx.context))
    }

    /// Painting the parts (or assemblies) `paths` at any depth, each in its own assembly.
    fn paint_paths(paths: &[EntityPath], cx: &Cx<'_>) -> Operation {
        let material = cx.material;
        cx.deep_op(paths, |entities| Operation::PaintOccurrences { context: Id(0), entities, material })
    }

    /// What the rectangle `m` takes: parts at any depth (« Parts »), else the active context's
    /// occurrences (its guides left out).
    fn hits(m: &Marquee, cx: &Cx<'_>) -> (Vec<Id>, Vec<EntityPath>) {
        if cx.parts {
            return (Vec::new(), marquee_parts(cx, m.from, m.to, m.crossing));
        }
        let ids = marquee_hits(cx, m.from, m.to, m.crossing).into_iter().filter(|id| cx.is_occurrence(*id)).collect();
        (ids, Vec::new())
    }

    /// The rectangle `from`–`to` released: what it takes painted, in one step.
    fn paint_rectangle(from: glam::DVec2, to: glam::DVec2, cx: &Cx<'_>) -> Vec<Command> {
        let (ids, paths) = Self::hits(&Marquee::new(from, to), cx);
        if !paths.is_empty() {
            return vec![Command::Apply(Self::paint_paths(&paths, cx))];
        }
        if ids.is_empty() {
            return vec![];
        }
        vec![Command::Apply(Operation::PaintOccurrences { context: cx.context_def, entities: ids, material: cx.material })]
    }

    fn paint(&self, hit: &Hit, cx: &Cx<'_>) -> Result<Operation, String> {
        if faces(cx) {
            let (part, _, _) = cx.doc.resolve_part(&hit.path).map_err(|e| e.to_string())?;
            return Ok(Operation::PaintFaces { part, faces: vec![hit.face], material: cx.material });
        }
        if cx.parts {
            let path = Self::part(hit, cx).ok_or_else(|| t!("tool.paint.outside"))?;
            let paths = if cx.deep.contains(&path) { cx.deep.to_vec() } else { vec![path] };
            return Ok(Self::paint_paths(&paths, cx));
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
        self.faces = faces(cx);
        match input {
            ToolInput::Hover { px } => self.hover = if self.marquee.is_some() { None } else { cx.pick(*px) },
            ToolInput::DragStart { px, .. } => {
                self.hover = None;
                self.marquee = Some(Marquee::new(*px, *px));
            }
            ToolInput::Drag { px } => {
                if let Some(m) = &self.marquee {
                    let mut m = Marquee::new(m.from, *px);
                    (m.hits, m.paths) = Self::hits(&m, cx);
                    self.marquee = Some(m);
                }
            }
            ToolInput::DragEnd { px, mods } => {
                // Esc during the drag dropped it.
                let Some(m) = self.marquee.take() else { return vec![] };
                if (*px - m.from).abs().max_element() < MIN_MARQUEE {
                    return self.input(&ToolInput::Click { px: *px, mods: *mods }, cx);
                }
                return Self::paint_rectangle(m.from, *px, cx);
            }
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
        if let Some(m) = &self.marquee {
            return Preview { marquee: Some(m.clone()), ..Default::default() };
        }
        let Some(hit) = &self.hover else { return Preview::default() };
        if faces(cx) {
            // A face belongs to the part: every occurrence of it changes.
            let path: EntityPath = hit.path.clone();
            return Preview { hover_solid: Some(path.clone()), propagate: Some(path), ..Default::default() };
        }
        if cx.parts {
            return Preview { hover_solid: Self::part(hit, cx), ..Default::default() };
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
                OptionChoice {
                    value: OCCURRENCE,
                    label: t!("tool.paint.option.occurrence"),
                    tip: t!("tool.paint.option.occurrence_tip"),
                    icon: None,
                },
                OptionChoice { value: FACE, label: t!("tool.paint.option.face"), tip: t!("tool.paint.option.face_tip"), icon: None },
            ],
            saved: true,
            icon: None,
        }]
    }

    fn set_option(&mut self, key: &str, value: &OptionValue) {
        if key == TARGET {
            self.faces = matches!(value, OptionValue::Choice(v) if v == FACE);
        }
    }

    fn status(&self) -> String {
        match &self.marquee {
            Some(m) if m.crossing => t!("tool.select.hint.crossing"),
            Some(_) => t!("tool.select.hint.window"),
            None if self.faces => t!("tool.paint.hint"),
            None => t!("tool.paint.hint_rectangle"),
        }
    }

    /// Painting faces, a drag paints where it is released; occurrences, it draws a rectangle.
    fn drag_clicks(&self) -> DragClicks {
        if self.faces { DragClicks::Release } else { DragClicks::No }
    }

    fn busy(&self) -> bool {
        self.marquee.is_some()
    }

    /// Painting occurrences only: a face is a part's, at any depth already.
    fn picks_parts(&self) -> bool {
        !self.faces
    }

    /// The level chosen in the tree drop-up is painted, not selected (§6 septendecies).
    fn tree_pick(&self, path: &EntityPath, cx: &Cx<'_>) -> Option<Vec<Command>> {
        Some(vec![Command::Apply(Self::paint_paths(std::slice::from_ref(path), cx))])
    }

    fn leave(&mut self) {
        self.hover = None;
    }

    fn reset(&mut self) {
        self.hover = None;
        self.marquee = None;
    }
}

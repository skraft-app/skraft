//! Paint (SketchUp's paint bucket, OpenCutList's Smart Paint, docs/render/look.md): a click paints
//! the occurrence under the cursor — the whole selection when it is in it — or a single face of
//! its part, with the current material (none: the paint is removed). The option says which. Under
//! « Parts » (docs/tools/selection.md), the part under the cursor at any depth, in its assembly —
//! the parts selected when it is one of them —, and the level chosen in the tree drop-up.
//! Painting occurrences, a drag draws a rectangle: what it encloses (left to right) or touches
//! (right to left) is painted, at once when it is released (one step); Esc drops it. What a click
//! would paint is drawn in the colour it would take. The other modifier held (Command on macOS,
//! Alt elsewhere) holds « Prélever » over it (`sample.rs`, the tool stack,
//! docs/tools/interaction.md#tool-stack).

use super::DragClicks;
use skraft_i18n::t;
use skraft_model::{EntityPath, Id, Operation};

use super::select::{marquee_hits, marquee_parts};
use super::{
    Command, Cx, Marquee, OptionChoice, OptionValue, PaintPreview, PaintTarget, Preview, Tool, ToolInput, ToolKind, ToolOption, alt_key,
};
use crate::scene::Hit;

/// The target option's key and values.
pub const TARGET: &str = "target";
const OCCURRENCE: &str = "occurrence";
const FACE: &str = "face";

/// A drag shorter than this (pixels, both ways) is a click.
const MIN_MARQUEE: f64 = 3.0;

#[derive(Default)]
pub struct PaintTool {
    hover: Option<Hit>,
    /// The rectangle dragged (painting occurrences).
    marquee: Option<Marquee>,
    /// Painting faces, as the option said at the last input.
    faces: bool,
    /// The other modifier held at the last input: pressed, it holds « Prélever »; still held
    /// once that hands back (Esc), it does not again.
    alt: bool,
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

    /// What the rectangle `m` (its hits worked out) paints, if anything.
    fn rectangle_target(m: &Marquee) -> Option<PaintTarget> {
        if !m.paths.is_empty() {
            Some(PaintTarget::Paths(m.paths.clone()))
        } else if !m.hits.is_empty() {
            Some(PaintTarget::Occurrences(m.hits.clone()))
        } else {
            None
        }
    }

    /// What a click on `hit` paints.
    fn target(&self, hit: &Hit, cx: &Cx<'_>) -> Result<PaintTarget, String> {
        if self.faces {
            return Ok(PaintTarget::Face { path: hit.path.clone(), face: hit.face });
        }
        if cx.parts {
            let path = Self::part(hit, cx).ok_or_else(|| t!("tool.paint.outside"))?;
            let paths = if cx.deep.contains(&path) { cx.deep.to_vec() } else { vec![path] };
            return Ok(PaintTarget::Paths(paths));
        }
        let id = cx.selectable(&hit.path).ok_or_else(|| t!("tool.paint.outside"))?;
        Ok(PaintTarget::Occurrences(if cx.selection.contains(&id) { cx.selection.to_vec() } else { vec![id] }))
    }

    /// The operation painting `target` with the current material.
    fn paint(target: PaintTarget, cx: &Cx<'_>) -> Result<Operation, String> {
        let material = cx.material;
        Ok(match target {
            PaintTarget::Occurrences(entities) => Operation::PaintOccurrences { context: cx.context_def, entities, material },
            PaintTarget::Paths(paths) => Self::paint_paths(&paths, cx),
            PaintTarget::Face { path, face } => {
                let (part, _, _) = cx.doc.resolve_part(&path).map_err(|e| e.to_string())?;
                Operation::PaintFaces { part, faces: vec![face], material }
            }
        })
    }
}

impl Tool for PaintTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Paint
    }

    fn cursor(&self, _cx: &Cx<'_>) -> Option<crate::tools::Cursor> {
        Some(if self.faces { crate::tools::Cursor::PaintFace } else { crate::tools::Cursor::PaintPart })
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        self.faces = cx.choice(TARGET) == Some(FACE);
        // The other modifier pressed: « Prélever » over it, until released (not while a
        // rectangle is dragged).
        let pressed = cx.alt_down && !self.alt;
        self.alt = cx.alt_down;
        if pressed && self.marquee.is_none() {
            self.hover = None;
            return vec![Command::Hold(ToolKind::Sample)];
        }
        match input {
            ToolInput::Hover { px } => self.hover = if self.marquee.is_some() { None } else { cx.pick(*px) },
            ToolInput::DragStart { px, .. } => {
                self.hover = None;
                self.marquee = Some(Marquee::new(*px, *px));
            }
            ToolInput::Drag { px } => {
                if let Some(m) = &self.marquee {
                    let mut m = Marquee { selects: false, ..Marquee::new(m.from, *px) };
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
                let mut m = Marquee::new(m.from, *px);
                (m.hits, m.paths) = Self::hits(&m, cx);
                return match Self::rectangle_target(&m).map(|t| Self::paint(t, cx)) {
                    Some(Ok(op)) => vec![Command::Apply(op)],
                    Some(Err(why)) => vec![Command::Say(why)],
                    None => vec![],
                };
            }
            ToolInput::Click { px, .. } => {
                let Some(hit) = cx.pick(*px) else { return vec![] };
                return match self.target(&hit, cx).and_then(|t| Self::paint(t, cx)) {
                    Ok(op) => vec![Command::Apply(op)],
                    Err(why) => vec![Command::Say(why)],
                };
            }
            _ => {}
        }
        vec![]
    }

    fn preview(&self, cx: &Cx<'_>) -> Preview {
        let paint = |target| Some(PaintPreview { material: cx.material, target });
        if let Some(m) = &self.marquee {
            return Preview { marquee: Some(m.clone()), paint: Self::rectangle_target(m).and_then(paint), ..Default::default() };
        }
        let Some(hit) = &self.hover else { return Preview::default() };
        let paint = self.target(hit, cx).ok().and_then(paint);
        if self.faces {
            // A face belongs to the part: every occurrence of it changes.
            let path: EntityPath = hit.path.clone();
            return Preview { hover_solid: Some(path.clone()), propagate: Some(path), paint, ..Default::default() };
        }
        if cx.parts {
            return Preview { hover_solid: Self::part(hit, cx), paint, ..Default::default() };
        }
        Preview { hover: cx.selectable(&hit.path), paint, ..Default::default() }
    }

    fn options(&self, cx: &Cx<'_>) -> Vec<ToolOption> {
        let value = if cx.choice(TARGET) == Some(FACE) { FACE } else { OCCURRENCE };
        let choice = |value, label, tip| OptionChoice { value, label, tip, icon: None };
        vec![ToolOption {
            key: TARGET,
            label: t!("tool.paint.option.target"),
            tip: t!("tool.paint.option.target_tip"),
            value: OptionValue::Choice(value.into()),
            choices: vec![
                choice(OCCURRENCE, t!("tool.paint.option.occurrence"), t!("tool.paint.option.occurrence_tip")),
                choice(FACE, t!("tool.paint.option.face"), t!("tool.paint.option.face_tip")),
            ],
            saved: true,
            icon: None,
            swatch: None,
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
            None if self.faces => t!("tool.paint.hint", alt_key = alt_key()),
            None => t!("tool.paint.hint_rectangle", alt_key = alt_key()),
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

    /// The level chosen in the tree drop-up is painted, not selected (docs/tools/selection.md).
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

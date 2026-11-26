//! Paint (SketchUp's paint bucket, OpenCutList's Smart Paint, docs/render/look.md): a click paints
//! the part under the cursor — every part of an assembly, the whole selection when it is in
//! it — or a single face of its part, with the current material (none: the paint is removed).
//! The option says which. A part's material is on its definition: the scope says whether its
//! other copies take it too, or the ones painted become a part of their own
//! (docs/decisions/0057-materials-on-the-definition-typed-sources.md). Under
//! « Parts » (docs/tools/selection.md), the part under the cursor at any depth, in its assembly —
//! the parts selected when it is one of them —, and the level chosen in the tree drop-up.
//! Painting parts, a drag draws a rectangle: what it encloses (left to right) or touches
//! (right to left) is painted, at once when it is released (one step); Esc drops it. What a click
//! would paint is drawn in the colour it would take. The other modifier held (Command on macOS,
//! Alt elsewhere) holds « Prélever » over it (`sample.rs`, the tool stack,
//! docs/tools/interaction.md#tool-stack).

use super::DragClicks;
use skraft_i18n::t;
use skraft_model::{EntityPath, Operation, Scope};

use super::select::{marquee_hits, marquee_parts};
use super::{
    Command, Cx, Marquee, OptionChoice, OptionValue, PaintPreview, PaintTarget, Preview, Tool, ToolInput, ToolKind, ToolOption, alt_key,
};
use crate::scene::Hit;

/// The target option's key and values.
pub const TARGET: &str = "target";
const PART: &str = "part";
const FACE: &str = "face";
/// The scope option's key and values.
pub const SCOPE: &str = "scope";
const ALL: &str = "all";
const THIS: &str = "this";

fn scope(cx: &Cx<'_>) -> Scope {
    if cx.choice(SCOPE) == Some(THIS) { Scope::This } else { Scope::All }
}

/// A drag shorter than this (pixels, both ways) is a click.
const MIN_MARQUEE: f64 = 3.0;

#[derive(Default)]
pub struct PaintTool {
    hover: Option<Hit>,
    /// The rectangle dragged (painting parts).
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
        Some(hit.path.part_path()).filter(|p| p.occurrences.starts_with(cx.context))
    }

    /// The occurrences `ids` of the active context, as paths from the root (its guides left
    /// out).
    fn paths_of(ids: &[skraft_model::Id], cx: &Cx<'_>) -> Vec<EntityPath> {
        ids.iter().filter(|id| cx.is_occurrence(**id)).map(|id| EntityPath::new(cx.context.to_vec(), *id)).collect()
    }

    /// The parts (or assemblies) `paths` painted.
    fn parts(paths: Vec<EntityPath>, cx: &Cx<'_>) -> PaintTarget {
        PaintTarget::Parts { paths, scope: scope(cx) }
    }

    /// What the rectangle `m` takes: parts at any depth (« Parts »), else the active context's
    /// occurrences (its guides left out).
    fn hits(m: &Marquee, cx: &Cx<'_>) -> (Vec<skraft_model::Id>, Vec<EntityPath>) {
        if cx.parts {
            return (Vec::new(), marquee_parts(cx, m.from, m.to, m.crossing));
        }
        let ids = marquee_hits(cx, m.from, m.to, m.crossing).into_iter().filter(|id| cx.is_occurrence(*id)).collect();
        (ids, Vec::new())
    }

    /// What the rectangle `m` (its hits worked out) paints, if anything.
    fn rectangle_target(m: &Marquee, cx: &Cx<'_>) -> Option<PaintTarget> {
        let paths = if m.paths.is_empty() { Self::paths_of(&m.hits, cx) } else { m.paths.clone() };
        (!paths.is_empty()).then(|| Self::parts(paths, cx))
    }

    /// What a click on `hit` paints.
    fn target(&self, hit: &Hit, cx: &Cx<'_>) -> Result<PaintTarget, String> {
        if self.faces {
            return Ok(PaintTarget::Face { path: hit.path.clone(), face: hit.face });
        }
        if cx.parts {
            let path = Self::part(hit, cx).ok_or_else(|| t!("tool.paint.outside"))?;
            let paths = if cx.deep.contains(&path) { cx.deep.to_vec() } else { vec![path] };
            return Ok(Self::parts(paths, cx));
        }
        let id = cx.selectable(&hit.path).ok_or_else(|| t!("tool.paint.outside"))?;
        let ids = if cx.selection.contains(&id) { cx.selection.to_vec() } else { vec![id] };
        Ok(Self::parts(Self::paths_of(&ids, cx), cx))
    }

    /// The operation painting `target` with the current material.
    fn paint(target: PaintTarget, cx: &Cx<'_>) -> Result<Operation, String> {
        let material = cx.material;
        Ok(match target {
            PaintTarget::Parts { paths, scope } => Operation::PaintParts { targets: paths, material, scope },
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
                return match Self::rectangle_target(&m, cx).map(|t| Self::paint(t, cx)) {
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
            return Preview { marquee: Some(m.clone()), paint: Self::rectangle_target(m, cx).and_then(paint), ..Default::default() };
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
        let faces = cx.choice(TARGET) == Some(FACE);
        let choice = |value, label, tip| OptionChoice { value, label, tip, icon: None };
        let mut out = vec![ToolOption {
            key: TARGET,
            label: t!("tool.paint.option.target"),
            tip: t!("tool.paint.option.target_tip"),
            value: OptionValue::Choice(if faces { FACE } else { PART }.into()),
            choices: vec![
                choice(PART, t!("tool.paint.option.part"), t!("tool.paint.option.part_tip")),
                choice(FACE, t!("tool.paint.option.face"), t!("tool.paint.option.face_tip")),
            ],
            saved: true,
            icon: None,
            swatch: None,
        }];
        // A face is painted on every copy of its part: no scope. Nor for a part placed once:
        // both choices are the same (docs/model/data-model.md).
        let aimed = self.hover.as_ref().and_then(|h| Self::part(h, cx));
        let all = match aimed.map(|p| cx.copies_shown(&p)) {
            Some(n) if n > 1 => Some(t!("tool.paint.option.all_count", count = n)),
            Some(_) => None,
            None => Some(t!("tool.paint.option.all")),
        };
        if let (false, Some(all)) = (faces, all) {
            let value = if scope(cx) == Scope::This { THIS } else { ALL };
            out.push(ToolOption {
                key: SCOPE,
                label: t!("tool.paint.option.scope"),
                tip: t!("tool.paint.option.scope_tip"),
                value: OptionValue::Choice(value.into()),
                choices: vec![
                    OptionChoice { value: ALL, label: all, tip: t!("tool.paint.option.all_tip"), icon: Some("scope_all") },
                    OptionChoice {
                        value: THIS,
                        label: t!("tool.paint.option.this"),
                        tip: t!("tool.paint.option.this_tip"),
                        icon: Some("scope_this"),
                    },
                ],
                saved: true,
                icon: None,
                swatch: None,
            });
        }
        out
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

    /// Painting faces, a drag paints where it is released; parts, it draws a rectangle.
    fn drag_clicks(&self) -> DragClicks {
        if self.faces { DragClicks::Release } else { DragClicks::No }
    }

    fn busy(&self) -> bool {
        self.marquee.is_some()
    }

    /// Painting parts only: a face is a part's, at any depth already.
    fn picks_parts(&self) -> bool {
        !self.faces
    }

    /// The level chosen in the tree drop-up is painted, not selected (docs/tools/selection.md).
    fn tree_pick(&self, path: &EntityPath, cx: &Cx<'_>) -> Option<Vec<Command>> {
        let op = Operation::PaintParts { targets: vec![path.clone()], material: cx.material, scope: scope(cx) };
        Some(vec![Command::Apply(op)])
    }

    fn leave(&mut self) {
        self.hover = None;
    }

    fn reset(&mut self) {
        self.hover = None;
        self.marquee = None;
    }
}

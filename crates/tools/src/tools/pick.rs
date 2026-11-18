//! Picking what a tool acts on, before it acts (docs/tools/selection.md#select-first),
//! as OpenCutList's `SmartSelectActionHandler`: the selection when there is one; else a click
//! takes the entity under the cursor, Shift clicks add or remove (the set taken once Shift is
//! released), a rectangle takes what it encloses (left to right) or touches (right to left).
//! What is taken becomes the selection. Move, Rotate, the copy tools and the Stretch share it.

use glam::DVec2;
use skraft_i18n::t;
use skraft_model::{EntityPath, GuidePath, Id};

use super::select::{marquee_guides, marquee_hits, marquee_parts, toggled, toggled_paths};
use super::{Command, Cx, Marquee, Preview, ToolInput};

/// The least a rectangle is dragged to be one (pixels).
const MIN_MARQUEE: f64 = 3.0;

/// What a tool may take: every entity a click of Select takes, or only some (the Stretch
/// takes occurrences, not guides).
pub(super) type Keep = fn(&Cx<'_>, Vec<Id>) -> Vec<Id>;

/// Everything.
fn all(_: &Cx<'_>, ids: Vec<Id>) -> Vec<Id> {
    ids
}

#[derive(Clone, Debug)]
pub(super) struct Picker {
    /// Shift held after a Shift click or rectangle (OCL's `STATE_SELECT_MULTIPLE`): the
    /// selection is being made, ignored until Shift is released.
    pub gathering: bool,
    pub marquee: Option<Marquee>,
    /// What a click would take.
    pub hovered: Option<Id>,
    /// The part a click would take at any depth (« Parts », docs/tools/selection.md).
    pub hovered_part: Option<EntityPath>,
    /// The guide a click would take at any depth (« Parts »).
    pub hovered_guide: Option<GuidePath>,
    /// What was taken is let go (the copies made, Esc): the selection, still drawn, is not the
    /// target until something is picked again (the copy tools' OCL restart).
    dropped: bool,
    keep: Keep,
    /// Does it take guides (« Parts »: those at any depth)? Not when `keep` is given.
    guides: bool,
}

impl Default for Picker {
    fn default() -> Self {
        Picker {
            gathering: false,
            marquee: None,
            hovered: None,
            hovered_part: None,
            hovered_guide: None,
            dropped: false,
            keep: all,
            guides: true,
        }
    }
}

impl Picker {
    /// A picker taking only what `keep` leaves (no guides at any depth).
    pub fn keeping(keep: Keep) -> Self {
        Picker { keep, guides: false, ..Default::default() }
    }

    /// The entity a click at `px` takes, if the tool takes it.
    fn under(&self, cx: &Cx<'_>, px: DVec2) -> Option<Id> {
        // A guide near the cursor not taken: the solid under it.
        let solid = || cx.pick(px).and_then(|h| cx.selectable(&h.path));
        let ids: Vec<Id> = cx.entity_at(px).into_iter().chain(solid()).collect();
        (self.keep)(cx, ids).first().copied()
    }

    fn hits(&self, cx: &Cx<'_>, m: &Marquee) -> Vec<Id> {
        (self.keep)(cx, marquee_hits(cx, m.from, m.to, m.crossing))
    }

    /// The guide at any depth a click at `px` takes under « Parts », if the tool takes guides.
    fn guide_under(&self, cx: &Cx<'_>, px: DVec2) -> Option<GuidePath> {
        if self.guides { cx.guide_at(px) } else { None }
    }

    /// The guides at any depth the rectangle takes under « Parts », if the tool takes guides.
    fn guide_hits(&self, cx: &Cx<'_>, m: &Marquee) -> Vec<GuidePath> {
        if self.guides { marquee_guides(cx, m.from, m.to, m.crossing) } else { Vec::new() }
    }

    /// Shift released while gathering: the selection made is taken. Returns whether it was.
    pub fn shift_released(&mut self, cx: &Cx<'_>) -> bool {
        if self.gathering && !cx.shift_down {
            self.gathering = false;
            self.dropped = false;
            return true;
        }
        false
    }

    /// Is the selection the target now: not let go ([`Picker::drop`]), nor being gathered?
    pub fn aims(&self) -> bool {
        !self.dropped && !self.gathering
    }

    /// What was taken let go: the tool picks again, the selection no longer its target.
    pub fn drop(&mut self) {
        self.dropped = true;
    }

    /// Was what was taken let go (Esc then lets the selection go too, [`super::Tool::dropped`])?
    pub fn dropped(&self) -> bool {
        self.dropped
    }

    /// Is the selection what the tool acts on now (something selected, here or at any depth,
    /// not being gathered)?
    pub fn taken(&self, cx: &Cx<'_>) -> bool {
        !self.gathering
            && (!cx.deep.is_empty() || (self.guides && !cx.deep_guides.is_empty()) || !(self.keep)(cx, cx.selection.to_vec()).is_empty())
    }

    /// The parts selected at any depth the tool acts on (« Parts »).
    pub fn deep(&self, cx: &Cx<'_>) -> Vec<EntityPath> {
        if self.gathering { Vec::new() } else { cx.deep.to_vec() }
    }

    /// The guides selected at any depth the tool acts on (« Parts »).
    pub fn deep_guides(&self, cx: &Cx<'_>) -> Vec<GuidePath> {
        if self.gathering || !self.guides { Vec::new() } else { cx.deep_guides.to_vec() }
    }

    /// The selection the tool acts on, as it may take it.
    pub fn selection(&self, cx: &Cx<'_>) -> Vec<Id> {
        if self.gathering { Vec::new() } else { (self.keep)(cx, cx.selection.to_vec()) }
    }

    /// Is a rectangle dragged, or the selection being gathered?
    pub fn busy(&self) -> bool {
        self.marquee.is_some() || self.gathering
    }

    /// The commands for `input`, and whether a target was taken by it.
    pub fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> (Vec<Command>, bool) {
        let (out, took) = if cx.parts { self.input_parts(input, cx) } else { self.input_context(input, cx) };
        if took {
            self.dropped = false;
        }
        (out, took)
    }

    /// [`Picker::input`] under « Context »: the active context's entities.
    fn input_context(&mut self, input: &ToolInput, cx: &Cx<'_>) -> (Vec<Command>, bool) {
        match input {
            ToolInput::Hover { px } => self.hovered = self.under(cx, *px),
            ToolInput::Click { px, mods } if mods.toggle => {
                if let Some(id) = self.under(cx, *px) {
                    self.gathering = true;
                    return (vec![Command::Select(toggled(cx.selection, &[id]))], false);
                }
            }
            ToolInput::Click { px, .. } => {
                if let Some(id) = self.under(cx, *px) {
                    self.hovered = None;
                    return (vec![Command::Select(vec![id])], true);
                }
            }
            ToolInput::DragStart { px, .. } => {
                self.hovered = None;
                self.marquee = Some(Marquee::new(*px, *px));
            }
            ToolInput::Drag { px } => {
                if let Some(m) = &self.marquee {
                    let mut m = Marquee::new(m.from, *px);
                    m.hits = self.hits(cx, &m);
                    self.marquee = Some(m);
                }
            }
            ToolInput::DragEnd { px, mods } => {
                let Some(m) = self.marquee.take() else { return (vec![], false) };
                if (*px - m.from).abs().max_element() < MIN_MARQUEE {
                    return self.input_context(&ToolInput::Click { px: *px, mods: *mods }, cx);
                }
                let hits = self.hits(cx, &Marquee::new(m.from, *px));
                if mods.toggle {
                    self.gathering = true;
                    return (vec![Command::Select(toggled(cx.selection, &hits))], false);
                }
                let took = !hits.is_empty();
                return (vec![Command::Select(hits)], took);
            }
            _ => {}
        }
        (vec![], false)
    }

    /// [`Picker::input`] under « Parts » (docs/tools/selection.md): parts at any depth, and the
    /// active context's guides.
    fn input_parts(&mut self, input: &ToolInput, cx: &Cx<'_>) -> (Vec<Command>, bool) {
        match input {
            ToolInput::Hover { px } => {
                self.hovered_guide = self.guide_under(cx, *px);
                self.hovered_part = if self.hovered_guide.is_none() { cx.part_at(*px) } else { None };
            }
            ToolInput::Click { px, mods } if mods.toggle => {
                if let Some(g) = self.guide_under(cx, *px) {
                    self.gathering = true;
                    return (vec![Command::SelectPaths(cx.deep.to_vec(), toggled_paths(cx.deep_guides, &[g]))], false);
                }
                if let Some(p) = cx.part_at(*px) {
                    self.gathering = true;
                    return (vec![Command::SelectPaths(toggled_paths(cx.deep, &[p]), cx.deep_guides.to_vec())], false);
                }
            }
            ToolInput::Click { px, .. } => {
                if let Some(g) = self.guide_under(cx, *px) {
                    self.hovered_guide = None;
                    return (vec![Command::SelectPaths(Vec::new(), vec![g])], true);
                }
                if let Some(p) = cx.part_at(*px) {
                    self.hovered_part = None;
                    return (vec![Command::SelectPaths(vec![p], Vec::new())], true);
                }
            }
            ToolInput::DragStart { px, .. } => {
                self.hovered_guide = None;
                self.hovered_part = None;
                self.marquee = Some(Marquee::new(*px, *px));
            }
            ToolInput::Drag { px } => {
                if let Some(m) = &self.marquee {
                    let mut m = Marquee::new(m.from, *px);
                    m.paths = marquee_parts(cx, m.from, m.to, m.crossing);
                    m.guides = self.guide_hits(cx, &m);
                    self.marquee = Some(m);
                }
            }
            ToolInput::DragEnd { px, mods } => {
                let Some(m) = self.marquee.take() else { return (vec![], false) };
                if (*px - m.from).abs().max_element() < MIN_MARQUEE {
                    return self.input_parts(&ToolInput::Click { px: *px, mods: *mods }, cx);
                }
                let m = Marquee::new(m.from, *px);
                let paths = marquee_parts(cx, m.from, m.to, m.crossing);
                let guides = self.guide_hits(cx, &m);
                if mods.toggle {
                    self.gathering = true;
                    let guides = toggled_paths(cx.deep_guides, &guides);
                    return (vec![Command::SelectPaths(toggled_paths(cx.deep, &paths), guides)], false);
                }
                let took = !paths.is_empty() || !guides.is_empty();
                return (vec![Command::SelectPaths(paths, guides)], took);
            }
            _ => {}
        }
        (vec![], false)
    }

    /// What picking shows: the entity a click would take, the rectangle.
    pub fn preview(&self, p: &mut Preview) {
        p.hover = self.hovered;
        p.hover_solid = self.hovered_part.clone();
        p.hover_guide = self.hovered_guide.clone();
        p.marquee = self.marquee.clone();
    }

    /// The status while picking: `pick`, `gather` while the selection is being gathered, the
    /// rectangle's while one is dragged.
    pub fn status(&self, pick: String, gather: String) -> String {
        match &self.marquee {
            Some(m) if m.crossing => t!("tool.select.hint.crossing"),
            Some(_) => t!("tool.select.hint.window"),
            None if self.gathering => gather,
            None => pick,
        }
    }

    /// One step back: the rectangle dropped. Returns whether there was one.
    pub fn back(&mut self) -> bool {
        self.marquee.take().is_some()
    }

    /// The cursor left the view.
    pub fn leave(&mut self) {
        self.hovered = None;
        self.hovered_part = None;
        self.hovered_guide = None;
    }
}

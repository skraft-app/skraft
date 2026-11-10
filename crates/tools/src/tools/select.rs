//! Select: click, Shift+click, double-click to enter an assembly, double-click in the
//! void (or outside the context) to leave it. Dragging draws a selection rectangle (SketchUp):
//! left to right it takes the entities it encloses entirely (window), right to left those it
//! touches (crossing); Shift toggles them instead of replacing the selection.

use std::collections::HashMap;

use glam::{DVec2, DVec3};
use solidsketch_i18n::t;
use solidsketch_model::Id;

use super::{Command, Cx, Marquee, Modifiers, Preview, Tool, ToolInput, ToolKind};
use crate::camera::Projector;

/// A drag shorter than this (pixels, both ways) is a click.
const MIN_MARQUEE: f64 = 3.0;

#[derive(Default)]
pub struct SelectTool {
    hover: Option<Id>,
    marquee: Option<Marquee>,
}

impl SelectTool {
    fn click(&self, px: DVec2, mods: Modifiers, cx: &Cx<'_>) -> Vec<Command> {
        match cx.pick(px).map(|h| cx.selectable(&h.path)) {
            Some(Some(id)) if mods.toggle => vec![Command::Select(toggled(cx.selection, &[id]))],
            Some(Some(id)) => vec![Command::Select(vec![id])],
            // Outside the context, or the void.
            _ if mods.toggle => vec![],
            _ => vec![Command::Select(vec![])],
        }
    }
}

/// `selection` with each of `ids` removed if in it, else added.
fn toggled(selection: &[Id], ids: &[Id]) -> Vec<Id> {
    let mut sel = selection.to_vec();
    for id in ids {
        match sel.iter().position(|s| s == id) {
            Some(i) => {
                sel.remove(i);
            }
            None => sel.push(*id),
        }
    }
    sel
}

impl Tool for SelectTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Select
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        let target = |px: DVec2| cx.pick(px).map(|h| cx.selectable(&h.path));
        match input {
            ToolInput::Hover { px } => {
                self.hover = if self.marquee.is_some() { None } else { target(*px).flatten() };
                vec![]
            }
            ToolInput::Click { px, mods } => self.click(*px, *mods, cx),
            ToolInput::DoubleClick { px, .. } => match target(*px) {
                Some(Some(id)) => {
                    if cx.doc.is_assembly_occurrence(cx.context_def, id) { vec![Command::Enter(id)] } else { vec![] }
                }
                _ if !cx.context.is_empty() => vec![Command::Exit],
                _ => vec![],
            },
            ToolInput::DragStart { px, .. } => {
                self.hover = None;
                self.marquee = Some(Marquee::new(*px, *px));
                vec![]
            }
            ToolInput::Drag { px } => {
                if let Some(m) = &mut self.marquee {
                    *m = Marquee::new(m.from, *px);
                    m.hits = marquee_hits(cx, m.from, m.to, m.crossing);
                }
                vec![]
            }
            ToolInput::DragEnd { px, mods } => {
                // Esc during the drag dropped it.
                let Some(m) = self.marquee.take() else { return vec![] };
                if ((*px - m.from).abs()).max_element() < MIN_MARQUEE {
                    return self.click(*px, *mods, cx);
                }
                let m = Marquee::new(m.from, *px);
                let hits = marquee_hits(cx, m.from, m.to, m.crossing);
                if mods.toggle { vec![Command::Select(toggled(cx.selection, &hits))] } else { vec![Command::Select(hits)] }
            }
            ToolInput::Key { .. } | ToolInput::Vcb(_) | ToolInput::Value(_) => vec![],
        }
    }

    fn preview(&self, _: &Cx<'_>) -> Preview {
        Preview { hover: self.hover, marquee: self.marquee.clone(), ..Default::default() }
    }

    fn status(&self) -> String {
        match &self.marquee {
            Some(m) if m.crossing => t!("tool.select.hint.crossing"),
            Some(_) => t!("tool.select.hint.window"),
            None => t!("tool.select.hint.idle"),
        }
    }

    fn busy(&self) -> bool {
        self.marquee.is_some()
    }

    fn reset(&mut self) {
        self.hover = None;
        self.marquee = None;
    }
}

/// The entities of the active context the rectangle `a`–`b` (pixels) takes: those whose every
/// part lies entirely inside it, or with `crossing`, those any part of which it touches. In
/// the order of the scene.
pub fn marquee_hits(cx: &Cx<'_>, a: DVec2, b: DVec2, crossing: bool) -> Vec<Id> {
    let rect = (a.min(b), a.max(b));
    let proj = cx.camera.projector(cx.size);
    // Per entity: are all its parts inside, does any touch?
    let mut order = Vec::new();
    let mut state: HashMap<Id, (bool, bool)> = HashMap::new();
    for (placement, part) in cx.scene.parts() {
        let Some(id) = cx.selectable(&placement.entity_path()) else { continue };
        let m = &part.mesh;
        let world = |p: &[f32; 3]| placement.transform.transform_point3(m.origin + DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64));
        let s = state.entry(id).or_insert_with(|| {
            order.push(id);
            (true, false)
        });
        if crossing {
            if !s.1 && part_touches(&proj, m, &world, rect) {
                s.1 = true;
            }
        } else if s.0 {
            s.0 = !m.positions.is_empty()
                && m.positions.iter().all(|p| proj.project(world(p)).is_some_and(|q| inside(q, rect)));
        }
    }
    order.into_iter().filter(|id| state.get(id).is_some_and(|&(all, any)| if crossing { any } else { all })).collect()
}

fn part_touches(proj: &Projector, m: &solidsketch_kernel::Mesh, world: &dyn Fn(&[f32; 3]) -> DVec3, rect: (DVec2, DVec2)) -> bool {
    let screen: Vec<Option<DVec2>> = m.positions.iter().map(|p| proj.project(world(p))).collect();
    // Triangles; a stand-in (a box outline) has edges only.
    if m.indices.is_empty() {
        let at = |p: &[f32; 3]| proj.project(world(p));
        return m.edges.iter().any(|e| {
            e.windows(2).any(|w| matches!((at(&w[0]), at(&w[1])), (Some(p), Some(q)) if segment_hits(p, q, rect)))
        });
    }
    let center = (rect.0 + rect.1) * 0.5;
    m.indices.chunks_exact(3).any(|t| {
        let v = |i: u32| screen.get(i as usize).copied().flatten();
        // A triangle partly behind the eye is left out.
        let (Some(p), Some(q), Some(r)) = (v(t[0]), v(t[1]), v(t[2])) else { return false };
        segment_hits(p, q, rect) || segment_hits(q, r, rect) || segment_hits(r, p, rect) || in_triangle(center, p, q, r)
    })
}

fn inside(p: DVec2, (lo, hi): (DVec2, DVec2)) -> bool {
    p.cmpge(lo).all() && p.cmple(hi).all()
}

/// Does the segment `a`–`b` meet the rectangle (Liang–Barsky clipping)?
fn segment_hits(a: DVec2, b: DVec2, (lo, hi): (DVec2, DVec2)) -> bool {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for k in 0..2 {
        for (p, q) in [(-d[k], a[k] - lo[k]), (d[k], hi[k] - a[k])] {
            if p == 0.0 {
                if q < 0.0 {
                    return false;
                }
            } else {
                let r = q / p;
                if p < 0.0 { t0 = t0.max(r) } else { t1 = t1.min(r) }
                if t0 > t1 {
                    return false;
                }
            }
        }
    }
    true
}

fn in_triangle(p: DVec2, a: DVec2, b: DVec2, c: DVec2) -> bool {
    let side = |u: DVec2, v: DVec2| (v - u).perp_dot(p - u);
    let (s0, s1, s2) = (side(a, b), side(b, c), side(c, a));
    (s0 >= 0.0 && s1 >= 0.0 && s2 >= 0.0) || (s0 <= 0.0 && s1 <= 0.0 && s2 <= 0.0)
}

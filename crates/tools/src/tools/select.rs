//! Select: click, Shift+click, double-click to enter an assembly or open a part
//! (ARCHITECTURE.md §2.9), a click off its grey box to leave it. In an open part showing its
//! machinings, clicks take its machinings' tools. Dragging draws a selection rectangle (SketchUp):
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
        let target = if let Some(g) = cx.pick_guide(px) {
            Some(Some(g))
        } else if cx.machinings_shown() {
            Some(cx.pick_machining(px))
        } else {
            cx.pick(px).map(|h| cx.selectable(&h.path))
        };
        match target {
            Some(Some(id)) if mods.toggle => vec![Command::Select(toggled(cx.selection, &[id]))],
            Some(Some(id)) => vec![Command::Select(vec![id])],
            // Outside the context, or the void: off its box, it is left.
            _ if mods.toggle => vec![],
            _ => vec![Command::ClickOff(px)],
        }
    }
}

/// `selection` with each of `ids` removed if in it, else added.
pub(crate) fn toggled(selection: &[Id], ids: &[Id]) -> Vec<Id> {
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
        let target = |px: DVec2| {
            cx.pick_guide(px)
                .or_else(|| if cx.machinings_shown() { cx.pick_machining(px) } else { cx.pick(px).and_then(|h| cx.selectable(&h.path)) })
        };
        match input {
            ToolInput::Hover { px } => {
                self.hover = if self.marquee.is_some() { None } else { target(*px) };
                vec![]
            }
            ToolInput::Click { px, mods } => self.click(*px, *mods, cx),
            // Its first click has already left the context when off its box: a double-click
            // only enters (one level left per double-click, as per click).
            ToolInput::DoubleClick { px, .. } => match cx.pick(*px).and_then(|h| cx.selectable(&h.path)) {
                Some(id) => vec![Command::Enter(id)],
                None => vec![],
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
/// the order of the scene, then its guides.
pub fn marquee_hits(cx: &Cx<'_>, a: DVec2, b: DVec2, crossing: bool) -> Vec<Id> {
    let rect = (a.min(b), a.max(b));
    let proj = cx.camera.projector(cx.size);
    // Only a part whose box, seen, meets the rectangle can touch it or lie in it.
    let meets = |b: &(DVec3, DVec3)| screen_box(&proj, b).is_none_or(|(lo, hi)| lo.cmple(rect.1).all() && hi.cmpge(rect.0).all());
    // Per entity: are all its parts met inside, does any touch, how many are met?
    let mut met = Vec::new();
    let mut state: HashMap<Id, (bool, bool, usize)> = HashMap::new();
    for (_, placement, part) in cx.scene.parts_where(meets) {
        let Some(id) = cx.selectable(&placement.entity_path()) else { continue };
        let m = &part.mesh;
        let world = |p: &[f32; 3]| placement.transform.transform_point3(m.origin + DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64));
        let s = state.entry(id).or_insert_with(|| {
            met.push(id);
            (true, false, 0)
        });
        s.2 += 1;
        if crossing {
            if !s.1 && part_touches(&proj, m, &world, rect) {
                s.1 = true;
            }
        } else if s.0 {
            s.0 = !m.positions.is_empty() && m.positions.iter().all(|p| proj.project(world(p)).is_some_and(|q| inside(q, rect)));
        }
    }
    // Inside the rectangle: every part of the entity, those not met included. In the scene's
    // order (of each entity's first part).
    let mut taken: Vec<(usize, Id)> = met
        .into_iter()
        .filter_map(|id| {
            let (all, any, n) = state[&id];
            let chain: Vec<Id> = cx.context.iter().copied().chain([id]).collect();
            let (total, first) = cx.scene.parts_under(&chain);
            let ok = if crossing { any } else { all && n == total };
            ok.then_some((first.unwrap_or(usize::MAX), id))
        })
        .collect();
    taken.sort_by_key(|(rank, _)| *rank);
    let mut out: Vec<Id> = taken.into_iter().map(|(_, id)| id).collect();
    // The guides of the active context (§6 quinquies).
    let path = cx.guide_path();
    for g in cx.scene.guides().iter().filter(|g| g.path == path) {
        let (all, any) = crate::guides::in_rect(cx.camera, cx.size, &g.shape, rect);
        if if crossing { any } else { all } {
            out.push(g.id);
        }
    }
    out
}

/// The screen rectangle the box `b` (world) covers; none when a corner is behind the eye.
fn screen_box(proj: &Projector, (lo, hi): &(DVec3, DVec3)) -> Option<(DVec2, DVec2)> {
    let mut out: Option<(DVec2, DVec2)> = None;
    for i in 0..8 {
        let c =
            DVec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z });
        let s = proj.project(c)?;
        out = Some(out.map_or((s, s), |(a, b)| (a.min(s), b.max(s))));
    }
    out
}

fn part_touches(proj: &Projector, m: &solidsketch_kernel::Mesh, world: &dyn Fn(&[f32; 3]) -> DVec3, rect: (DVec2, DVec2)) -> bool {
    let screen: Vec<Option<DVec2>> = m.positions.iter().map(|p| proj.project(world(p))).collect();
    // Triangles; a stand-in (a box outline) has edges only.
    if m.indices.is_empty() {
        let at = |p: &[f32; 3]| proj.project(world(p));
        return m
            .edges
            .iter()
            .any(|e| e.windows(2).any(|w| matches!((at(&w[0]), at(&w[1])), (Some(p), Some(q)) if segment_hits(p, q, rect))));
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
                if p < 0.0 {
                    t0 = t0.max(r)
                } else {
                    t1 = t1.min(r)
                }
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

#[cfg(test)]
mod tests {
    use glam::{DAffine3, DVec2, DVec3};
    use solidsketch_model::{Document, Id, Operation, Primitive};

    use super::*;
    use crate::{Editor, StandardView, ViewArea};

    /// What the rectangle takes, testing every part of the scene (the index must agree).
    fn marquee_by_scan(cx: &Cx<'_>, a: DVec2, b: DVec2, crossing: bool) -> Vec<Id> {
        let rect = (a.min(b), a.max(b));
        let proj = cx.camera.projector(cx.size);
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
                s.0 = !m.positions.is_empty() && m.positions.iter().all(|p| proj.project(world(p)).is_some_and(|q| inside(q, rect)));
            }
        }
        order.into_iter().filter(|id| state.get(id).is_some_and(|&(all, any)| if crossing { any } else { all })).collect()
    }

    #[test]
    fn the_rectangle_takes_what_a_scan_takes() {
        // Boxes of several sizes in a grid, a few gathered in an assembly (copied), one of
        // them holding an assembly in turn: entities of one part or of several, far apart.
        let mut doc = Document::new();
        let root = doc.root();
        let mut made = Vec::new();
        for i in 0..10 {
            for j in 0..10 {
                let size = DVec3::new(40.0 + (i % 3) as f64 * 20.0, 50.0, 30.0 + (j % 2) as f64 * 40.0);
                let at = DVec3::new(i as f64 * 90.0, j as f64 * 90.0, 0.0);
                let primitive = Primitive::Box { origin: at, size };
                let (d, out) = doc.apply(&Operation::AddPrimitive { context: root, primitive, placement: DAffine3::IDENTITY }).unwrap();
                doc = d;
                made.push(out.created[0]);
            }
        }
        let (d, inner) = doc.apply(&Operation::MakeAssembly { context: root, entities: vec![made[0], made[55]], name: None }).unwrap();
        let (d, outer) =
            d.apply(&Operation::MakeAssembly { context: root, entities: vec![inner.created[0], made[9], made[90]], name: None }).unwrap();
        let outer = outer.created[0];
        doc = d
            .apply(&Operation::Duplicate { context: root, entities: vec![outer], transform: DAffine3::from_translation(DVec3::Z * 150.0) })
            .unwrap()
            .0;
        let mut e = Editor::new(doc, Box::new(solidsketch_kernel::Inline::default()));
        e.set_viewport_size(DVec2::new(800.0, 600.0));
        let scene = (DVec3::ZERO, DVec3::new(900.0, 900.0, 250.0));
        let mut seed = 7u64;
        let mut next = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as f64 / (1u64 << 31) as f64
        };
        let mut taken = 0;
        // At the model's level, then inside the assembly (its occurrences the entities).
        for (view, parallel, inside) in [
            (StandardView::Iso, false, false),
            (StandardView::Top, true, false),
            (StandardView::Front, false, false),
            (StandardView::Iso, false, true),
        ] {
            if inside {
                e.enter(outer);
                assert_eq!(e.context(), &[outer]);
            }
            e.set_parallel(parallel);
            e.camera.set_standard_view(view, Some(scene), ViewArea::full(e.viewport_size()));
            let cx = e.cx();
            for _ in 0..60 {
                let a = DVec2::new(next() * 800.0, next() * 600.0);
                let b = DVec2::new(next() * 800.0, next() * 600.0);
                for crossing in [false, true] {
                    let found = marquee_hits(&cx, a, b, crossing);
                    assert_eq!(found, marquee_by_scan(&cx, a, b, crossing), "{view:?} {a} {b} {crossing}");
                    taken += found.len();
                }
            }
        }
        assert!(taken > 100, "{taken}");
    }
}

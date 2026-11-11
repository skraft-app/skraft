//! What the viewport draws this frame: the parts' placements, the highlights, the tools'
//! lines and the inferred point.

use glam::{DAffine3, DVec3};
use solidsketch_model::{Id, Section};

use super::Editor;
use crate::infer::InferKind;
use crate::scene::{solids_under, transform_bounds};
use crate::tools::{self, Marquee, PreviewLine};

/// How a highlighted solid is marked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Selected,
    Hover,
    /// Picked by a tool (the boolean's target).
    Picked,
    /// The selection where the move tool would put it.
    Ghost,
    /// Another occurrence of the part a change is aimed at: it changes too (§2.5).
    Related,
}

/// How a placement is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Look {
    /// A part's shape.
    #[default]
    Shape,
    /// An open part's blank, its machinings shown: a ghost (ARCHITECTURE.md §2.9).
    Blank,
    /// One of its machinings' tools.
    Tool,
    /// A tool that failed to cut.
    FailedTool,
}

/// One occurrence of a definition to draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewPlacement {
    pub transform: DAffine3,
    /// Outside the context being edited (drawn faded).
    pub faded: bool,
    /// The material its faces take.
    pub material: Option<Id>,
    pub look: Look,
}

/// One solid of one occurrence drawn again, marked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewHighlight {
    /// The definition whose mesh holds the solid.
    pub definition: Id,
    pub solid: Id,
    pub transform: DAffine3,
    pub mark: Mark,
    /// The material the part takes there.
    pub material: Option<Id>,
}

/// What the viewport draws this frame (meshes come from [`Editor::scene`]).
#[derive(Clone, Debug, Default)]
pub struct View {
    pub batches: Vec<(Id, Vec<ViewPlacement>)>,
    pub highlights: Vec<ViewHighlight>,
    pub overlay: Vec<PreviewLine>,
    /// The inferred point to mark, with its kind (the app draws the marker and the tooltip).
    pub inference: Option<(DVec3, InferKind)>,
    /// The selection rectangle being dragged (the app draws it).
    pub marquee: Option<Marquee>,
    /// The section to draw: a tool's preview, else the document's active one.
    pub section: Option<Section>,
    /// The bounding boxes of the selected assemblies' content, dashed segments in the world
    /// (the app draws them depth-tested, in the selection's colour).
    pub selection_boxes: Vec<(DVec3, DVec3)>,
    /// The same box for the part open or the assembly being edited, grown by a few pixels
    /// (drawn in grey).
    pub context_box: Vec<(DVec3, DVec3)>,
}

/// How far the box of the assembly being edited stands off its content, in pixels.
const CONTEXT_BOX_OFFSET_PX: f64 = 8.0;

/// A box in its own axes: `frame` (rigid) maps them to the world.
#[derive(Clone, Copy, Debug)]
pub(super) struct OrientedBox {
    pub frame: DAffine3,
    pub lo: DVec3,
    pub hi: DVec3,
}

impl OrientedBox {
    /// Does `ray` pass through the box (slab test in its axes)?
    pub fn hit_by(&self, ray: &crate::camera::Ray) -> bool {
        let inv = self.frame.inverse();
        let (o, d) = (inv.transform_point3(ray.origin), inv.transform_vector3(ray.dir));
        let (mut t0, mut t1) = (ray.t_min, f64::INFINITY);
        for k in 0..3 {
            if d[k].abs() < 1e-12 {
                if o[k] < self.lo[k] || o[k] > self.hi[k] {
                    return false;
                }
                continue;
            }
            let (a, b) = ((self.lo[k] - o[k]) / d[k], (self.hi[k] - o[k]) / d[k]);
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
            if t0 > t1 {
                return false;
            }
        }
        true
    }
}

impl Editor {
    // ---- drawing -------------------------------------------------------------------------------

    /// A dashed line from `a` to `b` (dashes of a few pixels at their depth), dashes at both ends.
    pub(super) fn dashed(&self, out: &mut Vec<PreviewLine>, a: DVec3, b: DVec3, color: [f32; 4]) {
        let len = (b - a).length();
        let step = self.camera.world_per_pixel(self.size, (a + b) * 0.5) * 10.0;
        if len < 1e-12 || step <= 0.0 {
            return;
        }
        // Odd, so that a dash starts and ends the line (a box's corners stay drawn).
        let n = ((len / step).ceil() as usize).min(400) | 1;
        for k in (0..n).step_by(2) {
            let (t0, t1) = (k as f64 / n as f64, ((k + 1) as f64 / n as f64).min(1.0));
            out.push(PreviewLine { a: a.lerp(b, t0), b: a.lerp(b, t1), color });
        }
    }

    /// The box bounding what the occurrence `id` of the context `ctx_path` holds, in its own
    /// axes, moved by `extra` and grown by `offset_px` pixels on every side: its frame (to
    /// world) and its bounds in it. For a part's occurrence only when `parts` (the open
    /// part's box): its blank's.
    fn content_bounds(&self, ctx_path: &[Id], id: Id, extra: DAffine3, offset_px: f64, parts: bool) -> Option<OrientedBox> {
        let doc = self.document();
        let ctx = doc.context_of(ctx_path).ok()?;
        if !parts && !doc.is_assembly_occurrence(ctx, id) {
            return None;
        }
        let t = doc.transform_of(ctx_path).ok()?;
        let o = doc.assembly(ctx).ok().and_then(|a| a.occurrence(id))?;
        let frame = extra * t * o.transform;
        let to_local = frame.inverse();
        let mut bounds: Option<(DVec3, DVec3)> = None;
        for u in solids_under(doc, ctx_path, id) {
            // A part's recipe shown: its blank holds the tools' cuts.
            let key = self.scene.recipe().filter(|r| r.part == u.part).map_or(u.part, |r| r.blank);
            let Some(dm) = self.scene.definition_mesh(key) else { continue };
            let rel = to_local * extra * u.transform;
            for b in dm.parts.iter().filter_map(|p| p.bounds) {
                let (lo, hi) = transform_bounds(&rel, b);
                bounds = Some(bounds.map_or((lo, hi), |(l, h)| (l.min(lo), h.max(hi))));
            }
        }
        let (mut lo, mut hi) = bounds?;
        if offset_px > 0.0 {
            // Pixels measured at the box's centre (the frame is rigid: no scale).
            let d = self.camera.world_per_pixel(self.size, frame.transform_point3((lo + hi) * 0.5)) * offset_px;
            (lo, hi) = (lo - DVec3::splat(d), hi + DVec3::splat(d));
        }
        Some(OrientedBox { frame, lo, hi })
    }

    /// A box's edges, as dashed segments.
    fn dashed_box(&self, out: &mut Vec<(DVec3, DVec3)>, OrientedBox { frame, lo, hi }: OrientedBox) {
        let corner = |i: usize| {
            frame.transform_point3(DVec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            ))
        };
        let mut lines = Vec::new();
        for i in 0..8 {
            for bit in [1, 2, 4] {
                if i & bit == 0 {
                    self.dashed(&mut lines, corner(i), corner(i | bit), [0.0; 4]);
                }
            }
        }
        out.extend(lines.into_iter().map(|l| (l.a, l.b)));
    }

    /// The grey box drawn around the part open, else the assembly being edited (`None` at the
    /// model's level).
    pub(super) fn context_bounds(&self) -> Option<OrientedBox> {
        match (self.open_part, self.context.split_last()) {
            (Some(id), _) => self.content_bounds(&self.context, id, DAffine3::IDENTITY, CONTEXT_BOX_OFFSET_PX, true),
            (None, Some((&id, parent))) => self.content_bounds(parent, id, DAffine3::IDENTITY, CONTEXT_BOX_OFFSET_PX, false),
            (None, None) => None,
        }
    }

    /// What to draw now.
    pub fn view(&self) -> View {
        let mut v = View::default();
        let doc = self.document();
        let preview = self.preview();
        // Moved or turned without copying, the selection is only drawn where it goes: the
        // original stays in place only when a copy is left behind.
        let hide_original = preview.ghost.is_some() && !self.tool_copying();
        let depth = self.context.len();
        let open = self.open_part_path();
        // An open part showing its machinings is drawn as its recipe instead.
        let recipe = self.scene.recipe().filter(|_| self.showing_machinings());
        let hidden = |p: &&crate::scene::Placement| {
            (hide_original && p.path.starts_with(&self.context) && p.path.get(depth).is_some_and(|id| self.selection.contains(id)))
                || (recipe.is_some() && open.as_ref().is_some_and(|o| p.entity_path() == *o))
        };
        for (def, places) in self.scene.placements() {
            let ps = places
                .iter()
                .filter(|p| !hidden(p))
                .map(|p| ViewPlacement {
                    transform: p.transform,
                    faded: match &open {
                        Some(o) => p.entity_path() != *o,
                        None => !p.path.starts_with(&self.context),
                    },
                    material: p.material,
                    look: Look::Shape,
                })
                .collect();
            v.batches.push((def, ps));
        }
        if let (Some(r), Some(o)) = (recipe, &open)
            && let Ok((_, _, t)) = doc.resolve_part(o)
        {
            let material = self
                .scene
                .placements()
                .find(|(p, _)| *p == r.part)
                .and_then(|(_, places)| places.iter().find(|p| p.entity_path() == *o).and_then(|p| p.material));
            v.batches.push((r.blank, vec![ViewPlacement { transform: t, faded: false, material, look: Look::Blank }]));
            let mut tools: Vec<(Id, Vec<ViewPlacement>)> = Vec::new();
            // A tool moved without a copy is drawn only where it goes.
            for tool in r.tools.iter().filter(|tool| !(hide_original && self.selection.contains(&tool.machining))) {
                let look = if tool.failed { Look::FailedTool } else { Look::Tool };
                let p = ViewPlacement { transform: t * tool.placement, faded: false, material: None, look };
                match tools.iter_mut().find(|(k, _)| *k == tool.key) {
                    Some((_, ps)) => ps.push(p),
                    None => tools.push((tool.key, vec![p])),
                }
            }
            v.batches.extend(tools);
        }
        // While an open part's machinings show, the selection and hover are its machinings.
        let tool_at = |id: Id| {
            let r = recipe?;
            let t = doc.resolve_part(open.as_ref()?).ok()?.2;
            r.tools.iter().find(|tool| tool.machining == id).map(|tool| (tool.key, t * tool.placement))
        };
        let mark = |out: &mut Vec<ViewHighlight>, ctx: &[Id], id: Id, m: Mark, extra: DAffine3| {
            if recipe.is_some() {
                if let Some((key, t)) = tool_at(id) {
                    out.push(ViewHighlight { definition: key, solid: key, transform: extra * t, mark: m, material: None });
                }
                return;
            }
            for u in solids_under(doc, ctx, id) {
                out.push(ViewHighlight {
                    definition: u.part,
                    solid: u.part,
                    transform: extra * u.transform,
                    mark: m,
                    material: u.material,
                });
            }
        };
        for id in self.selection.iter().filter(|_| !hide_original) {
            mark(&mut v.highlights, &self.context, *id, Mark::Selected, DAffine3::IDENTITY);
        }
        // The selected assemblies' boxes, where the selection is drawn (moved with the ghost
        // when the original is hidden).
        let box_at = if hide_original { preview.ghost } else { Some(DAffine3::IDENTITY) };
        for (id, extra) in self.selection.iter().filter_map(|id| box_at.map(|g| (*id, g))) {
            if let Some(b) = self.content_bounds(&self.context, id, extra, 0.0, false) {
                self.dashed_box(&mut v.selection_boxes, b);
            }
        }
        // The part open, else the assembly being edited: its content's box, a little larger.
        if let Some(b) = self.context_bounds() {
            self.dashed_box(&mut v.context_box, b);
        }
        let listed = self.listed_hover.filter(|_| recipe.is_some());
        if let Some(h) = listed.or(preview.hover).filter(|h| !self.selection.contains(h)) {
            mark(&mut v.highlights, &self.context, h, Mark::Hover, DAffine3::IDENTITY);
        }
        // What the selection rectangle would take, as if hovered.
        for h in preview.marquee.iter().flat_map(|m| &m.hits).filter(|h| !self.selection.contains(h)) {
            mark(&mut v.highlights, &self.context, *h, Mark::Hover, DAffine3::IDENTITY);
        }
        for (path, m) in [(&preview.hover_solid, Mark::Hover), (&preview.picked_solid, Mark::Picked)] {
            if let Some(p) = path {
                mark(&mut v.highlights, &p.instances, p.entity, m, DAffine3::IDENTITY);
            }
        }
        // The other occurrences a change reaches, the one aimed at excepted: those of the open
        // part (§2.9), else of the part a tool aims at.
        let open_reaches_all = open.as_ref().filter(|_| self.part_scope == solidsketch_model::Scope::All);
        if let Some(aimed) = open_reaches_all.or(preview.propagate.as_ref())
            && let Ok((part, _, _)) = doc.resolve_part(aimed)
            && let Some((_, places)) = self.scene.placements().find(|(p, _)| *p == part)
        {
            for p in places.iter().filter(|p| p.entity_path() != *aimed) {
                v.highlights.push(ViewHighlight {
                    definition: part,
                    solid: part,
                    transform: p.transform,
                    mark: Mark::Related,
                    material: p.material,
                });
            }
        }
        if let Some(g) = preview.ghost {
            for id in &self.selection {
                mark(&mut v.highlights, &self.context, *id, Mark::Ghost, g);
            }
        }
        v.overlay = preview.lines;
        // Guides from tracked points, dashed (the tools draw those from their start point).
        if let Some(i) = &preview.inference {
            let color = tools::guide_color(i.kind);
            for (origin, _) in [i.line, i.line2].into_iter().flatten() {
                if self.tracked.contains(&origin) {
                    self.dashed(&mut v.overlay, origin, i.point, color);
                }
            }
        }
        v.inference = preview.inference.map(|i| (i.point, i.kind));
        v.marquee = preview.marquee;
        v.section = preview.section.or_else(|| doc.settings().display.active_section());
        v
    }
}

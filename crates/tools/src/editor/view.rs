//! What the viewport draws this frame: the parts' placements, the highlights, the tools'
//! lines and the inferred point.

use glam::{DAffine3, DVec3};
use solidsketch_model::{Id, Section};

use super::Editor;
use crate::guides::Stroke;
use crate::infer::InferKind;
use crate::scene::{ContextIndex, transform_bounds};
use crate::tools::{self, Marquee, PreviewFill, PreviewLine};

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
    /// A tool's lines drawn depth-tested.
    pub scene_lines: Vec<PreviewLine>,
    /// A tool's translucent polygons, drawn depth-tested.
    pub fills: Vec<PreviewFill>,
    /// The inferred point to mark, with its kind (the app draws the marker and the tooltip).
    pub inference: Option<(DVec3, InferKind)>,
    /// The selection rectangle being dragged (the app draws it).
    pub marquee: Option<Marquee>,
    /// The section to draw: a tool's preview, else the document's active one.
    pub section: Option<Section>,
    /// The bounding boxes of the selected occurrences (parts and assemblies), in their own
    /// axes (the app draws their edges dashed, depth-tested, in the selection's colour).
    pub selection_boxes: Vec<OrientedBox>,
    /// The same box for the part open or the assembly being edited, grown by a few pixels
    /// (drawn dashed, in grey); none at the model's level.
    pub context_box: Vec<OrientedBox>,
    /// The guides of the active context (ARCHITECTURE.md §6 quinquies), as strokes the app
    /// draws dashed and depth-tested.
    pub guides: Vec<Stroke>,
    /// Those of the other components, and of the open part's other occurrences (faded).
    pub faded_guides: Vec<Stroke>,
    /// The selected guides, where they are or where a move puts them.
    pub selected_guides: Vec<Stroke>,
    /// The guide under the cursor, or those the selection rectangle would take.
    pub hovered_guides: Vec<Stroke>,
    /// The guides a tool previews, drawn over everything, with their colour.
    pub overlay_guides: Vec<(Stroke, [f32; 4])>,
    /// The axes of the context (its frame's), as far as the view shows them: per axis, its
    /// positive half and its negative half (world segments, `None` out of view). The app draws
    /// them when they are shown.
    pub axes: [[Option<(DVec3, DVec3)>; 2]; 3],
}

/// How far the box of the assembly being edited stands off its content, in pixels.
const CONTEXT_BOX_OFFSET_PX: f64 = 8.0;

/// A box in its own axes: `frame` (rigid) maps them to the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrientedBox {
    pub frame: DAffine3,
    pub lo: DVec3,
    pub hi: DVec3,
}

impl OrientedBox {
    /// Its corners in the world, corner `i` taking `hi` along the axes of the bits of `i`.
    pub fn corners(&self) -> [DVec3; 8] {
        std::array::from_fn(|i| {
            let pick = |bit: usize, k: usize| if i & bit == 0 { self.lo[k] } else { self.hi[k] };
            self.frame.transform_point3(DVec3::new(pick(1, 0), pick(2, 1), pick(4, 2)))
        })
    }

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
        tools::dashed(&self.camera, self.size, out, a, b, color);
    }

    /// The box bounding what the occurrence `id` of the context `ctx_path` holds, in its own
    /// axes, moved by `extra` and grown by `offset_px` pixels on every side: its frame (to
    /// world) and its bounds in it (a part's recipe shown: its blank's).
    fn content_bounds(&self, ctx: &ContextIndex<'_>, id: Id, extra: DAffine3, offset_px: f64) -> Option<OrientedBox> {
        let o = ctx.occurrence(id)?;
        let frame = extra * ctx.transform() * o.transform;
        let to_local = frame.inverse();
        let mut bounds: Option<(DVec3, DVec3)> = None;
        for u in ctx.solids_under(id) {
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

    /// The grey box drawn around the part open, else the assembly being edited (`None` at the
    /// model's level).
    pub(super) fn context_bounds(&self) -> Option<OrientedBox> {
        match (self.open_part, self.context.split_last()) {
            (Some(id), _) => {
                self.content_bounds(&ContextIndex::new(self.document(), &self.context)?, id, DAffine3::IDENTITY, CONTEXT_BOX_OFFSET_PX)
            }
            (None, Some((&id, parent))) => {
                self.content_bounds(&ContextIndex::new(self.document(), parent)?, id, DAffine3::IDENTITY, CONTEXT_BOX_OFFSET_PX)
            }
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
        // Looked up for every placement: a set (a selection can hold tens of thousands).
        let selected: std::collections::HashSet<Id> = self.selection.iter().copied().collect();
        let depth = self.context.len();
        let open = self.open_part_path();
        // What a tool previews stretched (§6 sexies): each occurrence it changes drawn from its
        // preview mesh, or where it moves (`stretch_swaps`).
        let swaps = if preview.stretch.is_some() { Some(&self.stretch_swaps) } else { None };
        let swap = |p: &crate::scene::Placement| swaps.and_then(|s| s.get(&p.entity_path())).copied();
        // An open part showing its machinings is drawn as its recipe instead — but stretched,
        // as its shape.
        let recipe = self
            .scene
            .recipe()
            .filter(|_| self.showing_machinings())
            .filter(|_| open.as_ref().is_none_or(|o| swaps.is_none_or(|s| !s.contains_key(o))));
        let hidden = |p: &&crate::scene::Placement| {
            (hide_original && p.path.starts_with(&self.context) && p.path.get(depth).is_some_and(|id| selected.contains(id)))
                || (recipe.is_some() && open.as_ref().is_some_and(|o| p.entity_path() == *o))
        };
        let mut swapped: Vec<(Id, Vec<ViewPlacement>)> = Vec::new();
        // The highlights to move with them: from the part where it was to where it is drawn.
        let mut moved: Vec<(Id, DAffine3, Id, DAffine3)> = Vec::new();
        for (def, places) in self.scene.placements() {
            let mut ps = Vec::with_capacity(places.len());
            for p in places.iter().filter(|p| !hidden(p)) {
                let vp = ViewPlacement {
                    transform: p.transform,
                    faded: match &open {
                        Some(o) => p.entity_path() != *o,
                        None => !p.path.starts_with(&self.context),
                    },
                    material: p.material,
                    look: Look::Shape,
                };
                match swap(p) {
                    Some((key, t)) => {
                        moved.push((def, p.transform, key, t));
                        let vp = ViewPlacement { transform: t, ..vp };
                        match swapped.iter_mut().find(|(k, _)| *k == key) {
                            Some((_, ps)) => ps.push(vp),
                            None => swapped.push((key, vec![vp])),
                        }
                    }
                    None => ps.push(vp),
                }
            }
            v.batches.push((def, ps));
        }
        v.batches.extend(swapped);
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
            for tool in r.tools.iter().filter(|tool| !(hide_original && selected.contains(&tool.machining))) {
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
        // The occurrences of the context, indexed once, when something is marked.
        let index_cell = std::cell::OnceCell::new();
        let index = || index_cell.get_or_init(|| ContextIndex::new(doc, &self.context)).as_ref();
        let mark = |out: &mut Vec<ViewHighlight>, ctx: &[Id], id: Id, m: Mark, extra: DAffine3| {
            if recipe.is_some() {
                if let Some((key, t)) = tool_at(id) {
                    out.push(ViewHighlight { definition: key, solid: key, transform: extra * t, mark: m, material: None });
                }
                return;
            }
            let under = if ctx == self.context.as_slice() {
                index().map(|i| i.solids_under(id))
            } else {
                ContextIndex::new(doc, ctx).map(|i| i.solids_under(id))
            };
            for u in under.into_iter().flatten() {
                out.push(ViewHighlight {
                    definition: u.part,
                    solid: u.part,
                    transform: extra * u.transform,
                    mark: m,
                    material: u.material,
                });
            }
        };
        for id in self.selection.iter().filter(|_| !hide_original && !preview.hide_selection) {
            mark(&mut v.highlights, &self.context, *id, Mark::Selected, DAffine3::IDENTITY);
        }
        // The selected occurrences' boxes (parts and assemblies), where the selection is drawn (moved with the ghost
        // when the original is hidden). None with the stretch tool: it takes the selection as
        // what it stretches and draws its own box (grips, cutters), which this one would only
        // clutter — and leave behind, unstretched, while it previews.
        let box_at = if self.tool_kind() == tools::ToolKind::Stretch {
            None
        } else if hide_original {
            preview.ghost
        } else {
            Some(DAffine3::IDENTITY)
        };
        for (id, extra) in self.selection.iter().filter_map(|id| box_at.map(|g| (*id, g))) {
            if let Some(b) = index().and_then(|i| self.content_bounds(i, id, extra, 0.0)) {
                v.selection_boxes.push(b);
            }
        }
        // The part open, else the assembly being edited: its content's box, a little larger.
        v.context_box.extend(self.context_bounds());
        // The guides: the active context's (the open part's occurrence, else the context) —
        // selected, hovered or not —, and the others faded.
        let active: Vec<Id> = self.context.iter().copied().chain(self.open_part).collect();
        let marquee_hits: std::collections::HashSet<Id> = preview.marquee.iter().flat_map(|m| m.hits.iter().copied()).collect();
        let hovered = |id: Id| preview.hover == Some(id) || marquee_hits.contains(&id);
        for g in self.scene.guides() {
            let dashes = |shape: &solidsketch_model::GuideShape| crate::guides::strokes(&self.camera, self.size, shape);
            if g.path != active {
                v.faded_guides.extend(dashes(&g.shape));
                continue;
            }
            let is_selected = selected.contains(&g.id);
            if is_selected && let Some(t) = preview.ghost {
                v.selected_guides.extend(dashes(&g.shape.transformed(&t)));
                if !hide_original {
                    v.guides.extend(dashes(&g.shape));
                }
                continue;
            }
            let out = if is_selected {
                &mut v.selected_guides
            } else if hovered(g.id) {
                &mut v.hovered_guides
            } else {
                &mut v.guides
            };
            out.extend(dashes(&g.shape));
        }
        let listed = self.listed_hover.filter(|_| recipe.is_some());
        if let Some(h) = listed.or(preview.hover).filter(|h| !selected.contains(h)) {
            mark(&mut v.highlights, &self.context, h, Mark::Hover, DAffine3::IDENTITY);
        }
        // What the selection rectangle would take, as if hovered.
        for h in preview.marquee.iter().flat_map(|m| &m.hits).filter(|h| !selected.contains(h)) {
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
        // What is drawn stretched or moved is marked so (the selection's mark would draw it as
        // it was).
        for h in v.highlights.iter_mut() {
            if let Some((_, _, key, t)) = moved.iter().find(|(def, at, _, _)| *def == h.definition && at.abs_diff_eq(h.transform, 1e-9)) {
                (h.definition, h.solid, h.transform) = (*key, *key, *t);
            }
        }
        v.overlay = preview.lines;
        v.scene_lines = preview.scene_lines;
        v.fills = preview.fills;
        v.overlay_guides = preview.guides;
        // The axes, across the view.
        let frame = self.axes_transform();
        for (k, halves) in v.axes.iter_mut().enumerate() {
            let dir = frame.matrix3.col(k);
            for (half, range) in halves.iter_mut().zip([(0.0, f64::INFINITY), (f64::NEG_INFINITY, 0.0)]) {
                *half = crate::guides::visible_line(&self.camera, self.size, frame.translation, dir, range);
            }
        }
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

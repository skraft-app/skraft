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
    /// Hovered (in the outliner) outside the context being edited: hovered, still faded.
    HoverFaded,
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
    /// An open part's blank, its machinings shown (ARCHITECTURE.md §2.9).
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
    /// Drawn as a ghost — translucent faces, edges as they are —: an open part's blank, or
    /// what the eraser would take.
    pub ghost: bool,
    /// Its edges see-through too: what the eraser would take (the blank keeps its edges).
    pub faint_edges: bool,
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
    /// An open part's blank, drawn as a ghost: highlighted see-through (§2.11).
    pub ghost: bool,
}

/// What the viewport draws this frame (meshes come from [`Editor::scene`]).
#[derive(Clone, Debug, Default)]
pub struct View {
    pub batches: Vec<(Id, Vec<ViewPlacement>)>,
    pub highlights: Vec<ViewHighlight>,
    pub overlay: Vec<PreviewLine>,
    /// A tool's lines drawn depth-tested.
    pub scene_lines: Vec<PreviewLine>,
    /// A tool's lines plain where seen, dashed where faces hide them.
    pub xray: Vec<PreviewLine>,
    /// A tool's translucent polygons, drawn depth-tested.
    pub fills: Vec<PreviewFill>,
    /// The open part's profiled bodies' bottom caps, the cap the profile is drawn on (world
    /// segments): the app draws them bold, depth-tested, so that a see-through body shows
    /// which way it stands (§2.11).
    pub bases: Vec<(DVec3, DVec3)>,
    /// The axes of the open part's turned bodies (world segments, along their half profile):
    /// drawn dashed, depth-tested, with the bases (their half profiles, §6 undecies).
    pub base_axes: Vec<(DVec3, DVec3)>,
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
    /// The guides a tool previews, with their colour (drawn dashed and depth-tested, as the
    /// document's).
    pub preview_guides: Vec<(Stroke, [f32; 4])>,
    /// The axes of the context (its frame's), as far as the view shows them: per axis, its
    /// positive half and its negative half (world segments, `None` out of view). The app draws
    /// them when they are shown.
    pub axes: [[Option<(DVec3, DVec3)>; 2]; 3],
}

/// How the selection is drawn where it is — its solids marked, its occurrences' boxes —, and
/// the set of its ids: the same from one frame to the next while the scene (its generation),
/// the context, the selection and the recipe shown stay, worked out again only then. A move's
/// ghost is the same, moved.
pub(crate) struct SelectionDrawing {
    key: (u64, Vec<Id>, Vec<Id>, bool, bool),
    marks: Vec<ViewHighlight>,
    boxes: Vec<OrientedBox>,
    set: solidsketch_model::FastSet<Id>,
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
    pub(super) fn content_bounds(&self, ctx: &ContextIndex<'_>, id: Id, extra: DAffine3, offset_px: f64) -> Option<OrientedBox> {
        let o = ctx.occurrence(id)?;
        self.content_box(extra * ctx.transform() * o.transform, extra, &ctx.solids_under(id), offset_px)
    }

    /// The box bounding the body `id` of the open part (its recipe shown), in the body's axes,
    /// grown by `offset_px` pixels on every side.
    fn body_bounds(&self, id: Id, offset_px: f64) -> Option<OrientedBox> {
        let r = self.scene.recipe()?;
        let body = r.body(id)?;
        let (_, _, at) = self.document().resolve_container(&self.open_part_path()?).ok()?;
        let frame = at * body.placement;
        let dm = self.scene.definition_mesh(body.key)?;
        let (mut lo, mut hi) = dm.parts.iter().filter_map(|p| p.bounds).reduce(|(l, h), (lo, hi)| (l.min(lo), h.max(hi)))?;
        if offset_px > 0.0 {
            let d = self.camera.world_per_pixel(self.size, frame.transform_point3((lo + hi) * 0.5)) * offset_px;
            (lo, hi) = (lo - DVec3::splat(d), hi + DVec3::splat(d));
        }
        Some(OrientedBox { frame, lo, hi })
    }

    /// [`Editor::content_bounds`] for the parts `under` the occurrence (placed by
    /// [`ContextIndex::solids_under`]), its axes to world being `frame` (`extra` included).
    fn content_box(&self, frame: DAffine3, extra: DAffine3, under: &[crate::scene::Under], offset_px: f64) -> Option<OrientedBox> {
        let to_local = frame.inverse();
        let mut bounds: Option<(DVec3, DVec3)> = None;
        for u in under {
            // A part's recipe shown: its positive bodies hold the tools' cuts.
            let keys = match self.scene.recipe().filter(|r| r.part == u.part) {
                Some(r) => r.blanks.iter().map(|b| (b.key, r.within * b.placement)).collect(),
                None => vec![(u.part, DAffine3::IDENTITY)],
            };
            for (key, placement) in keys {
                let Some(dm) = self.scene.definition_mesh(key) else { continue };
                let rel = to_local * extra * u.transform * placement;
                for b in dm.parts.iter().filter_map(|p| p.bounds) {
                    let (lo, hi) = transform_bounds(&rel, b);
                    bounds = Some(bounds.map_or((lo, hi), |(l, h)| (l.min(lo), h.max(hi))));
                }
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

    /// The grey box drawn around the body open (its profile edited), else the part open, else
    /// the assembly being edited (`None` at the model's level).
    pub(super) fn context_bounds(&self) -> Option<OrientedBox> {
        if let Some(b) = self.profile_body().and_then(|id| self.body_bounds(id, CONTEXT_BOX_OFFSET_PX)) {
            return Some(b);
        }
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

    /// The selection drawn where it is ([`SelectionDrawing`]), for `key`.
    fn selection_drawing_of(&self, key: (u64, Vec<Id>, Vec<Id>, bool, bool)) -> SelectionDrawing {
        let doc = self.document();
        let mut marks = Vec::new();
        let mut boxes = Vec::new();
        // The recipe as the view shows it (`key.3`).
        let recipe = self.scene.recipe().filter(|_| key.3);
        if let Some(r) = recipe {
            // The open part's machinings: their tools, where the part is.
            let at = self.open_part_path().and_then(|o| doc.resolve_container(&o).ok()).map(|(_, _, t)| t);
            for id in &self.selection {
                if let (Some(t), Some(tool)) = (at, r.body(*id)) {
                    marks.push(ViewHighlight {
                        definition: tool.key,
                        solid: tool.key,
                        transform: t * tool.placement,
                        mark: Mark::Selected,
                        material: None,
                        ghost: key.4 || r.is_ghost(*id),
                    });
                }
            }
            // The other copies of the bodies selected (one definition, one mesh): a change to
            // the shape shows there too.
            let keys: Vec<Id> = self.selection.iter().filter_map(|id| r.body(*id)).map(|b| b.key).collect();
            for b in r.bodies().filter(|b| keys.contains(&b.key) && !self.selection.contains(&b.body)) {
                if let Some(t) = at {
                    marks.push(ViewHighlight {
                        definition: b.key,
                        solid: b.key,
                        transform: t * b.placement,
                        mark: Mark::Related,
                        material: None,
                        ghost: key.4 || r.is_ghost(b.body),
                    });
                }
            }
        }
        // Each occurrence's parts, found once for its marks and its box. The box, in the
        // occurrence's axes, is its component's: worked out once per component.
        let mut component_bounds: solidsketch_model::FastMap<Id, Option<(DVec3, DVec3)>> = Default::default();
        if let Some(index) = (!self.selection.is_empty()).then(|| ContextIndex::new(doc, &self.context)).flatten() {
            let mut under = Vec::new();
            // Neighbours often share their component: the last one's bounds first.
            let mut last: Option<(Id, Option<(DVec3, DVec3)>)> = None;
            for id in &self.selection {
                let Some(o) = index.occurrence(*id) else { continue };
                under.clear();
                index.solids_under_into(*id, &mut under);
                let frame = index.transform() * o.transform;
                let bounds = match last {
                    Some((c, b)) if c == o.component => b,
                    _ => *component_bounds
                        .entry(o.component)
                        .or_insert_with(|| self.content_box(frame, DAffine3::IDENTITY, &under, 0.0).map(|b| (b.lo, b.hi))),
                };
                last = Some((o.component, bounds));
                boxes.extend(bounds.map(|(lo, hi)| OrientedBox { frame, lo, hi }));
                if recipe.is_none() {
                    marks.extend(under.iter().map(|u| ViewHighlight {
                        definition: u.part,
                        solid: u.part,
                        transform: u.transform,
                        mark: Mark::Selected,
                        material: u.material,
                        ghost: key.4,
                    }));
                }
            }
        }
        let set = self.selection.iter().copied().collect();
        SelectionDrawing { key, marks, boxes, set }
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
        // The selection's marks, boxes and set (looked up for every placement: a selection can
        // hold tens of thousands), kept from the last frame when nothing they hang on changed.
        // X-ray (`key.4`): the marks drawn see-through.
        let key = (self.scene.generation(), self.context.clone(), self.selection.clone(), recipe.is_some(), self.xray);
        if self.selection_drawing.borrow().as_ref().is_none_or(|d| d.key != key) {
            let drawing = self.selection_drawing_of(key);
            *self.selection_drawing.borrow_mut() = Some(drawing);
        }
        let drawing = self.selection_drawing.borrow();
        let drawing = drawing.as_ref().expect("worked out above");
        let selected = &drawing.set;
        // Outside the context being edited: faded, or hidden with the rest of the model.
        let hide_rest_of_model = doc.settings().display.hide_rest_of_model;
        let outside = |p: &crate::scene::Placement| match &open {
            Some(o) => p.entity_path() != *o,
            None => !p.path.starts_with(&self.context),
        };
        // Hidden by the user: not drawn, or faded with the hidden shown.
        let show_hidden = doc.settings().display.show_hidden;
        let hidden = |p: &&crate::scene::Placement| {
            (hide_rest_of_model && outside(p))
                || (p.hidden && !show_hidden)
                || (hide_original && p.path.starts_with(&self.context) && p.path.get(depth).is_some_and(|id| selected.contains(id)))
                || (recipe.is_some() && open.as_ref().is_some_and(|o| p.entity_path() == *o))
        };
        // What the eraser would take: the context's occurrences (and the solids under them)
        // or the open part's machinings, drawn as ghosts, in batches of their own.
        let erasing: solidsketch_model::FastSet<Id> = preview.erasing.iter().copied().collect();
        let erased = |p: &crate::scene::Placement| {
            !erasing.is_empty()
                && open.is_none()
                && p.path.starts_with(&self.context)
                && p.path.get(depth).is_some_and(|id| erasing.contains(id))
        };
        let mut ghosts: Batches = Vec::new();
        let mut swapped: Vec<(Id, Vec<ViewPlacement>)> = Vec::new();
        // The highlights to move with them: from the part where it was to where it is drawn.
        let mut moved: Vec<(Id, DAffine3, Id, DAffine3)> = Vec::new();
        for (def, places) in self.scene.placements() {
            let mut ps = Vec::with_capacity(places.len());
            for p in places.iter().filter(|p| !hidden(p)) {
                let vp = ViewPlacement {
                    transform: p.transform,
                    faded: outside(p) || p.hidden,
                    material: p.material,
                    look: Look::Shape,
                    ghost: self.xray,
                    faint_edges: false,
                };
                if erased(p) {
                    push_to(&mut ghosts, def, ViewPlacement { ghost: true, faint_edges: true, ..vp });
                    continue;
                }
                match swap(p) {
                    Some((key, t)) => {
                        moved.push((def, p.transform, key, t));
                        push_to(&mut swapped, key, ViewPlacement { transform: t, ..vp });
                    }
                    None => ps.push(vp),
                }
            }
            v.batches.push((def, ps));
        }
        v.batches.extend(swapped);
        v.batches.extend(ghosts);
        if let (Some(r), Some(o)) = (recipe, &open)
            && let Ok((_, part, t)) = doc.resolve_container(o)
        {
            let material = self
                .scene
                .placements()
                .find(|(p, _)| *p == r.part)
                .and_then(|(_, places)| places.iter().find(|p| p.entity_path() == *o).and_then(|p| p.material));
            // A body stretched is drawn as previewed (§2.11).
            let body_swaps = if preview.stretch.is_some() { Some(&self.body_swaps) } else { None };
            let drawn = |b: &crate::scene::RecipeTool| body_swaps.and_then(|s| s.get(&b.body)).copied().unwrap_or((b.key, b.placement));
            // A body open (its profile edited): the others faded, as what lies outside any
            // context (§2.11 « Edit levels »).
            let open_body = self.profile_body();
            let faded = |id: Id| open_body.is_some_and(|o| o != id);
            // A positive body moved without a copy is drawn only where it goes, as a tool.
            for b in r.blanks.iter().filter(|b| !(hide_original && selected.contains(&b.body))) {
                let (key, placement) = drawn(b);
                let p = ViewPlacement {
                    transform: t * placement,
                    faded: faded(b.body),
                    material,
                    look: Look::Blank,
                    ghost: true,
                    faint_edges: erasing.contains(&b.body),
                };
                v.batches.push((key, vec![p]));
            }
            let (mut tools, mut tool_ghosts, mut tool_faint): (Batches, Batches, Batches) = (Vec::new(), Vec::new(), Vec::new());
            // A tool moved without a copy is drawn only where it goes.
            for tool in r.tools.iter().filter(|tool| !(hide_original && selected.contains(&tool.body))) {
                // A ghost, a little more opaque than a blank; one that failed opaque (§2.11).
                let look = if tool.failed { Look::FailedTool } else { Look::Tool };
                let faint = erasing.contains(&tool.body);
                let ghost = faint || !tool.failed || self.xray;
                let (key, placement) = drawn(tool);
                let p =
                    ViewPlacement { transform: t * placement, faded: faded(tool.body), material: None, look, ghost, faint_edges: faint };
                // Batches of one kind: a batch's edges are faint, or not, as a whole.
                let out = match (ghost, faint) {
                    (_, true) => &mut tool_faint,
                    (true, false) => &mut tool_ghosts,
                    (false, false) => &mut tools,
                };
                push_to(out, key, p);
            }
            // The machinings inside groups: faint ghosts, with their group (§2.11 « Body groups »).
            for tool in r
                .nested
                .iter()
                .filter(|n| !(hide_original && selected.contains(&n.body)) && !body_swaps.is_some_and(|s| s.contains_key(&n.body)))
            {
                let look = if tool.failed { Look::FailedTool } else { Look::Tool };
                let p = ViewPlacement {
                    transform: t * tool.placement,
                    faded: faded(tool.body),
                    material: None,
                    look,
                    ghost: true,
                    faint_edges: true,
                };
                push_to(&mut tool_faint, tool.key, p);
            }
            v.batches.extend(tools);
            v.batches.extend(tool_ghosts);
            v.batches.extend(tool_faint);
            // Each profiled body's bottom cap, where it is drawn: not one stretched (its
            // preview mesh no longer matches its profile), nor one moved (drawn elsewhere), nor
            // one faded.
            let stretched = |id: Id| body_swaps.is_some_and(|s| s.contains_key(&id));
            for rb in r.bodies().filter(|b| !stretched(b.body) && !(hide_original && selected.contains(&b.body)) && !faded(b.body)) {
                let Some(profile) = part.body(rb.body).and_then(|b| part.profile_of(b)) else { continue };
                let outline = profile.outline();
                let pts = solidsketch_model::kernel::profile::sample(&outline, BASE_STEP);
                let place = t * rb.placement;
                // An extrusion's bottom cap; a turned body's half profile in its XZ plane, and
                // its axis along it.
                let at = |q: glam::DVec2| match profile.extrusion() {
                    Some((z0, _)) => place.transform_point3(q.extend(z0)),
                    None => place.transform_point3(DVec3::new(q.x, 0.0, q.y)),
                };
                let n = pts.len();
                v.bases.extend((0..n).map(|i| (at(pts[i]), at(pts[(i + 1) % n]))));
                if profile.is_turned()
                    && let Some((lo, hi)) = solidsketch_model::kernel::profile::bounds(&outline)
                {
                    let over = (hi.y - lo.y) * 0.1;
                    v.base_axes.push((place.transform_point3(DVec3::Z * (lo.y - over)), place.transform_point3(DVec3::Z * (hi.y + over))));
                }
            }
        }
        // While an open part's machinings show, the selection and hover are its machinings.
        let tool_at = |id: Id| {
            let r = recipe?;
            let t = doc.resolve_container(open.as_ref()?).ok()?.2;
            r.body(id).map(|tool| (tool.key, t * tool.placement, self.xray || r.is_ghost(id)))
        };
        // The occurrences of the context, indexed once, when something is marked.
        let index_cell = std::cell::OnceCell::new();
        let index = || index_cell.get_or_init(|| ContextIndex::new(doc, &self.context)).as_ref();
        let mark = |out: &mut Vec<ViewHighlight>, ctx: &[Id], id: Id, m: Mark, extra: DAffine3| {
            if recipe.is_some() {
                if let Some((key, t, ghost)) = tool_at(id) {
                    out.push(ViewHighlight { definition: key, solid: key, transform: extra * t, mark: m, material: None, ghost });
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
                    ghost: self.xray,
                });
            }
        };
        if !hide_original && !preview.hide_selection {
            v.highlights.extend_from_slice(&drawing.marks);
        }
        // The selected occurrences' boxes (parts and assemblies), where the selection is drawn (moved with the ghost
        // when the original is hidden). None with the stretch and copy tools: they take the selection as
        // what it stretches and draws its own box (grips, cutters), which this one would only
        // clutter — and leave behind, unstretched, while it previews.
        let box_at = if matches!(
            self.tool_kind(),
            tools::ToolKind::Stretch | tools::ToolKind::CopyLine | tools::ToolKind::CopyGrid | tools::ToolKind::Distribute
        ) {
            None
        } else if hide_original {
            preview.ghost
        } else {
            Some(DAffine3::IDENTITY)
        };
        match box_at {
            Some(g) if g == DAffine3::IDENTITY => v.selection_boxes.extend_from_slice(&drawing.boxes),
            Some(g) => v.selection_boxes.extend(drawing.boxes.iter().map(|b| OrientedBox { frame: g * b.frame, ..*b })),
            None => {}
        }
        // The part open, else the assembly being edited: its content's box, a little larger.
        v.context_box.extend(self.context_bounds());
        // The guides: the active context's (the open part's occurrence, else the context) —
        // selected, hovered or not —, and the others faded.
        let active: Vec<Id> = self.context.iter().copied().chain(self.open_part).collect();
        let marquee_hits: solidsketch_model::FastSet<Id> = preview.marquee.iter().flat_map(|m| m.hits.iter().copied()).collect();
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
            // Copied by a copy tool: drawn at each copy too.
            if is_selected {
                for t in &preview.copies {
                    v.selected_guides.extend(dashes(&g.shape.transformed(t)));
                }
            }
            let out = if erasing.contains(&g.id) {
                &mut v.faded_guides
            } else if is_selected {
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
        // The occurrence hovered in the outliner, in any context, as if hovered in the view —
        // unless selected or open (already marked so) —, still faded outside the active path.
        if let Some(p) = &self.outliner_hover
            && open.as_ref() != Some(p)
            && !(p.instances == self.context && selected.contains(&p.entity))
            && let hovered = p.instances.iter().copied().chain([p.entity]).collect::<Vec<Id>>()
            // Hidden (it or an assembly above it, out of the active path): not marked, unless
            // the hidden are shown. What is hidden below it, `solids_under` leaves out.
            && (show_hidden || !crate::scene::is_chain_hidden(doc, &hovered, &active))
            && let Some(index) = ContextIndex::new(doc, &p.instances)
        {
            // An assembly holding the active path: only the solids under it are not faded.
            let inside = match active.split_last() {
                Some((last, above)) if !hovered.starts_with(&active) && active.starts_with(&hovered) => {
                    ContextIndex::new(doc, above).map(|i| i.solids_under(*last))
                }
                _ => None,
            };
            let all_inside = hovered.starts_with(&active);
            for u in index.solids_under(p.entity) {
                let faded = !all_inside
                    && inside.as_ref().is_none_or(|i| !i.iter().any(|w| w.part == u.part && w.transform.abs_diff_eq(u.transform, 1e-9)));
                v.highlights.push(ViewHighlight {
                    definition: u.part,
                    solid: u.part,
                    transform: u.transform,
                    mark: if faded { Mark::HoverFaded } else { Mark::Hover },
                    material: u.material,
                    ghost: self.xray,
                });
            }
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
            // Those not shown (the rest of the model hidden, or hidden by the user) are not marked either.
            let aimed = aimed.part_path();
            for p in places.iter().filter(|p| p.entity_path() != aimed && self.scene.is_shown(p)) {
                v.highlights.push(ViewHighlight {
                    definition: part,
                    solid: part,
                    transform: p.transform,
                    mark: Mark::Related,
                    material: p.material,
                    ghost: self.xray,
                });
            }
        }
        // The selection alone moves: not the other copies of its bodies' definitions (`Related`).
        for g in preview.ghost.iter().chain(&preview.copies) {
            let selection = drawing.marks.iter().filter(|h| h.mark == Mark::Selected);
            v.highlights.extend(selection.map(|h| ViewHighlight { transform: *g * h.transform, mark: Mark::Ghost, ..*h }));
        }
        // What the eraser would take is not marked (selected): a mark draws it opaque.
        if !erasing.is_empty() {
            let mut gone = Vec::new();
            for id in &erasing {
                mark(&mut gone, &self.context, *id, Mark::Hover, DAffine3::IDENTITY);
            }
            v.highlights.retain(|h| !gone.iter().any(|g| g.definition == h.definition && g.transform.abs_diff_eq(h.transform, 1e-9)));
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
        v.xray = preview.xray;
        v.fills = preview.fills;
        v.preview_guides = preview.guides;
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

/// How far an arc of a body's bottom cap turns between two points (radians).
const BASE_STEP: f64 = 0.06;

/// Placements by the mesh they draw.
type Batches = Vec<(Id, Vec<ViewPlacement>)>;

/// Adds `p` to the batch of `key` in `batches`, or to a new one.
fn push_to(batches: &mut Batches, key: Id, p: ViewPlacement) {
    match batches.iter_mut().find(|(k, _)| *k == key) {
        Some((_, ps)) => ps.push(p),
        None => batches.push((key, vec![p])),
    }
}

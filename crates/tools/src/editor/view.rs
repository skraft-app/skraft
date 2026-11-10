//! What the viewport draws this frame: the parts' placements, the highlights, the tools'
//! lines and the inferred point.

use glam::{DAffine3, DVec3};
use solidsketch_model::Id;

use super::Editor;
use crate::infer::InferKind;
use crate::scene::solids_under;
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

/// One occurrence of a definition to draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewPlacement {
    pub transform: DAffine3,
    /// Outside the context being edited (drawn faded).
    pub faded: bool,
}

/// One solid of one occurrence drawn again, marked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewHighlight {
    /// The definition whose mesh holds the solid.
    pub definition: Id,
    pub solid: Id,
    pub transform: DAffine3,
    pub mark: Mark,
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
}

impl Editor {
    // ---- drawing -------------------------------------------------------------------------------

    /// A dashed line from `a` to `b` (dashes of a few pixels at their depth).
    pub(super) fn dashed(&self, out: &mut Vec<PreviewLine>, a: DVec3, b: DVec3, color: [f32; 4]) {
        let len = (b - a).length();
        let step = self.camera.world_per_pixel(self.size, (a + b) * 0.5) * 10.0;
        if len < 1e-12 || step <= 0.0 {
            return;
        }
        let n = ((len / step).ceil() as usize).min(400);
        for k in (0..n).step_by(2) {
            let (t0, t1) = (k as f64 / n as f64, ((k + 1) as f64 / n as f64).min(1.0));
            out.push(PreviewLine { a: a.lerp(b, t0), b: a.lerp(b, t1), color });
        }
    }

    /// What to draw now.
    pub fn view(&self) -> View {
        let mut v = View::default();
        let doc = self.document();
        for (def, places) in self.scene.placements() {
            let ps = places
                .iter()
                .map(|p| ViewPlacement { transform: p.transform, faded: !p.path.starts_with(&self.context) })
                .collect();
            v.batches.push((def, ps));
        }
        let mark = |out: &mut Vec<ViewHighlight>, ctx: &[Id], id: Id, m: Mark, extra: DAffine3| {
            for (definition, solid, t) in solids_under(doc, ctx, id) {
                out.push(ViewHighlight { definition, solid, transform: extra * t, mark: m });
            }
        };
        let preview = self.preview();
        for id in &self.selection {
            mark(&mut v.highlights, &self.context, *id, Mark::Selected, DAffine3::IDENTITY);
        }
        if let Some(h) = preview.hover.filter(|h| !self.selection.contains(h)) {
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
        // The other occurrences a change reaches, the one aimed at excepted.
        if let Some(aimed) = &preview.propagate
            && let Ok((part, _, _)) = doc.resolve_part(aimed)
            && let Some((_, places)) = self.scene.placements().find(|(p, _)| *p == part)
        {
            for p in places.iter().filter(|p| p.entity_path() != *aimed) {
                v.highlights.push(ViewHighlight { definition: part, solid: part, transform: p.transform, mark: Mark::Related });
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
        v
    }
}

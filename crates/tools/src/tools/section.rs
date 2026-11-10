//! Section plane (§7 quater): click a face, the plane lies on it; move the cursor to sink it
//! into the model along the face's normal (or type the depth); click again to place it. What
//! lies outside of it — towards the face's outside — is cut away from the view. The cut shows
//! while the plane is being placed.

use glam::{DVec2, DVec3};
use solidsketch_i18n::t;
use solidsketch_model::{Operation, Section};

use super::{Command, Cx, Preview, PreviewLine, Tool, ToolInput, ToolKind};
use crate::vcb::{Vcb, VcbKind, VcbValue};

#[derive(Clone, Copy, Debug)]
struct Placing {
    /// The point clicked on the face, and the face's outward normal (world).
    origin: DVec3,
    normal: DVec3,
    /// How deep into the model the plane is (mm, along -normal).
    depth: f64,
}

impl Placing {
    fn section(&self) -> Section {
        Section { origin: self.origin - self.normal * self.depth, normal: self.normal }
    }
}

#[derive(Default)]
pub struct SectionTool {
    /// The face under the cursor: a point and its normal facing the eye (world).
    hover: Option<(DVec3, DVec3)>,
    placing: Option<Placing>,
}

impl SectionTool {
    fn commit(&mut self, cx: &Cx<'_>) -> Vec<Command> {
        let Some(p) = self.placing.take() else { return vec![] };
        let mut settings = cx.doc.settings().clone();
        settings.display.section = Some(p.section());
        settings.display.section_active = true;
        vec![Command::Apply(Operation::SetSettings(settings))]
    }
}

/// How far along the line `origin + t·dir` (unit) the ray through `px` passes closest.
fn along(cx: &Cx<'_>, px: DVec2, origin: DVec3, dir: DVec3) -> f64 {
    let r = cx.ray(px);
    let w = r.origin - origin;
    let (b, d, e) = (dir.dot(r.dir), dir.dot(w), r.dir.dot(w));
    let denom = 1.0 - b * b;
    if denom.abs() < 1e-9 {
        return 0.0; // looking along the line
    }
    (d - b * e) / denom
}

impl Tool for SectionTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Section
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        match input {
            ToolInput::Hover { px } => match &mut self.placing {
                Some(p) => p.depth = -along(cx, *px, p.origin, p.normal),
                None => self.hover = cx.pick(*px).map(|h| (h.point, h.normal)),
            },
            ToolInput::Click { px, .. } => {
                if self.placing.is_some() {
                    return self.commit(cx);
                }
                if let Some(h) = cx.pick(*px) {
                    self.placing = Some(Placing { origin: h.point, normal: h.normal, depth: 0.0 });
                }
            }
            ToolInput::Value(VcbValue::Lengths(v)) => {
                if let (Some(p), Some(d)) = (&mut self.placing, v.first()) {
                    p.depth = *d;
                    return self.commit(cx);
                }
            }
            _ => {}
        }
        vec![]
    }

    fn preview(&self, cx: &Cx<'_>) -> Preview {
        let (origin, normal, section) = match (&self.placing, &self.hover) {
            (Some(p), _) => (p.section().origin, p.normal, Some(p.section())),
            (None, Some((o, n))) => (*o, *n, None),
            (None, None) => return Preview::default(),
        };
        // A square on the plane, sized for the model.
        let half = cx.scene.bounds().map_or(100.0, |(lo, hi)| ((hi - lo).length() * 0.35).max(10.0));
        let u = normal.any_orthonormal_vector();
        let v = normal.cross(u);
        let c = |a: f64, b: f64| origin + (u * a + v * b) * half;
        let corners = [c(-1.0, -1.0), c(1.0, -1.0), c(1.0, 1.0), c(-1.0, 1.0)];
        let color = super::PICK;
        let mut lines: Vec<PreviewLine> =
            (0..4).map(|k| PreviewLine { a: corners[k], b: corners[(k + 1) % 4], color }).collect();
        // Its normal: towards what is cut away.
        lines.push(PreviewLine { a: origin, b: origin + normal * half * 0.3, color });
        Preview { lines, section, ..Default::default() }
    }

    fn status(&self) -> String {
        match self.placing {
            None => t!("tool.section.hint.face"),
            Some(_) => t!("tool.section.hint.depth"),
        }
    }

    fn vcb(&self) -> Option<Vcb> {
        self.placing.map(|p| Vcb::new(t!("tool.section.vcb.depth"), VcbKind::Lengths(vec![p.depth])))
    }

    fn busy(&self) -> bool {
        self.placing.is_some()
    }

    fn reset(&mut self) {
        self.placing = None;
    }
}

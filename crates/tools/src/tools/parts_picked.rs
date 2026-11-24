//! A set of parts a two-step tool picks — the solid tools' targets or tools, « Empreindre »'s
//! sources or destinations (docs/tools/solid-tools.md) —, each with the point it was taken at,
//! where its step's badge goes (docs/ui/interface.md#steps-bar).

use glam::DVec3;
use skraft_model::EntityPath;

use super::{Cx, StepBadge};

/// Once it holds more than one, or Shift added to it, it waits for Enter (OCL's `Selection`).
#[derive(Clone, Debug, Default)]
pub(super) struct PartsPicked {
    pub paths: Vec<EntityPath>,
    /// Where each was taken (world), in the order of `paths`.
    points: Vec<DVec3>,
    pub gathering: bool,
}

impl PartsPicked {
    /// Parts taken without a click (the selection): each at its box's centre.
    pub fn of(cx: &Cx<'_>, paths: Vec<EntityPath>) -> Self {
        let points = paths.iter().map(|p| centre(cx, p)).collect();
        PartsPicked { gathering: paths.len() > 1, paths, points }
    }

    /// `path` added, taken at `point`, or taken out if it was in.
    pub fn toggle(&mut self, path: EntityPath, point: DVec3) {
        if let Some(i) = self.paths.iter().position(|p| *p == path) {
            self.paths.remove(i);
            self.points.remove(i);
        } else {
            self.paths.push(path);
            self.points.push(point);
            self.gathering |= self.paths.len() > 1;
        }
    }

    pub fn clear(&mut self) {
        *self = PartsPicked::default();
    }

    /// The badges on its parts of the step named `name` ([`super::Steps::name`]).
    pub fn badges(&self, name: String) -> impl Iterator<Item = StepBadge> + '_ {
        self.points.iter().map(move |at| StepBadge { at: *at, name: name.clone() })
    }
}

/// The centre of the box of the part `path` names (world); its origin when it is not drawn.
fn centre(cx: &Cx<'_>, path: &EntityPath) -> DVec3 {
    cx.scene
        .part_at(path)
        .map(|(t, part)| t.transform_point3(part.bounds.map_or(DVec3::ZERO, |(lo, hi)| (lo + hi) / 2.0)))
        .unwrap_or_else(|| cx.doc.resolve_part(path).map(|(_, _, t)| t.translation).unwrap_or_default())
}

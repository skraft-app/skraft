//! Painting parts (docs/render/look.md, docs/decisions/0057-materials-on-the-definition-typed-sources.md):
//! a material goes on a part's definition, never on an occurrence nor an assembly. Painted
//! through occurrences, the scope says whether the part's other copies change too; an assembly
//! painted paints every part below it.

use crate::document::{BlendMode, Document, EntityPath, Family, Id, Seen};
use crate::ops::Scope;
use crate::{Error, FastMap, FastSet, Refusal, Result};

/// A coat as [`compose`] reads it: its blend, intensity, linear colour (a finish has no
/// texture).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coat {
    pub blend: BlendMode,
    pub intensity: f32,
    pub color: [f32; 3],
}

/// The coats `coats`, from the bottom up, composed per channel into one map of what is under,
/// `x -> A·x + B` (docs/decisions/0057-materials-on-the-definition-typed-sources.md): covering,
/// `(1 − t)·x + t·c`; tinting, `x·((1 − t) + t·c)`.
pub fn compose(coats: &[Coat]) -> ([f32; 3], [f32; 3]) {
    let (mut a, mut b) = ([1.0f32; 3], [0.0f32; 3]);
    for coat in coats {
        let t = coat.intensity.clamp(0.0, 1.0);
        let c = coat.color;
        match coat.blend {
            BlendMode::Cover => {
                for ch in 0..3 {
                    a[ch] *= 1.0 - t;
                    b[ch] = b[ch] * (1.0 - t) + t * c[ch];
                }
            }
            BlendMode::Tint => {
                for ch in 0..3 {
                    let m = 1.0 - t + t * c[ch];
                    a[ch] *= m;
                    b[ch] *= m;
                }
            }
        }
    }
    (a, b)
}

/// A part's placements, gathered by the material they are to take.
type Groups = Vec<(Option<Id>, Vec<EntityPath>)>;

impl Document {
    /// What `seen` looks like as it goes out of an export: its colour ([`Document::seen_srgb`]),
    /// named after what is under and the finish over it (« Chêne + Vernis »).
    pub fn seen_paint(&self, seen: Seen) -> Option<skraft_kernel::Paint> {
        let rgb = self.seen_srgb(seen)?;
        let name = |id: Option<Id>| id.and_then(|m| self.material(m).ok()).map(|m| m.name.as_str());
        let name = match (name(seen.under), name(seen.over)) {
            (Some(u), Some(o)) => format!("{u} + {o}"),
            (Some(n), None) | (None, Some(n)) => n.to_owned(),
            (None, None) => String::new(),
        };
        Some(skraft_kernel::Paint { name, rgb })
    }

    /// What `seen` looks like as one sRGB colour (docs/io/export.md): what is under's
    /// colour (a texture's own colour), its finish's coats composed over it as the viewport
    /// lays them; under nothing, over the light theme's face. Nothing at all: none.
    pub fn seen_srgb(&self, seen: Seen) -> Option<[u8; 3]> {
        use skraft_theme::{linear, srgb_byte};
        let under = seen.under.and_then(|m| self.material(m).ok()).map(|m| m.color.map(linear));
        let coats: Vec<Coat> = seen
            .over
            .map(|o| self.coats_of(o))
            .unwrap_or_default()
            .iter()
            .map(|(_, m)| Coat { blend: m.blend, intensity: m.intensity, color: m.color.map(linear) })
            .collect();
        if under.is_none() && coats.is_empty() {
            return None;
        }
        let face = skraft_theme::Theme::light().face_front;
        let x = under.unwrap_or([face[0], face[1], face[2]]);
        let (a, b) = compose(&coats);
        Some([0, 1, 2].map(|c| srgb_byte(x[c] * a[c] + b[c])))
    }

    /// Refuses `material` (if any) where it does not go: anywhere but `layer`, and never on
    /// `not` (the layer the operation does not reach).
    pub(crate) fn check_layer(&self, material: Option<Id>, layer: Family, not: Family) -> Result<()> {
        if layer == not {
            return Err(Error::Invalid(Refusal::MaterialLayer));
        }
        match material {
            Some(m) if self.material(m)?.family() != layer => Err(Error::Invalid(Refusal::MaterialLayer)),
            _ => Ok(()),
        }
    }

    /// Paints `layer` (a part's material or its finish) with `material` (`None`: unpaints) of the parts `targets` lead to from the root, an
    /// assembly's every part below it at any depth, hidden ones included. Under « only this
    /// one », a part some of whose placements are not painted gets a copy for those that are
    /// (one per part, as [`Document::separate`] makes it).
    pub(crate) fn paint_parts(&mut self, targets: &[EntityPath], layer: Family, material: Option<Id>, scope: Scope) -> Result<()> {
        let mut by_part: Vec<(Id, Vec<EntityPath>)> = Vec::new();
        let mut at: FastMap<Id, usize> = FastMap::default();
        let mut seen: FastSet<EntityPath> = FastSet::default();
        for t in targets {
            let component = self.resolve(t)?.component;
            let placed = match self.part(component) {
                Ok(_) => vec![(t.clone(), component)],
                Err(_) => {
                    let (mut path, mut out) = (t.occurrences.clone(), Vec::new());
                    path.push(t.entity);
                    self.walk(component, &mut path, glam::DAffine3::IDENTITY, None, &mut out, t.occurrences.len() + 1);
                    out.into_iter().map(|p| (p.path, p.part)).collect()
                }
            };
            for (path, part) in placed {
                if !seen.insert(path.clone()) {
                    continue;
                }
                let k = *at.entry(part).or_insert_with(|| {
                    by_part.push((part, Vec::new()));
                    by_part.len() - 1
                });
                by_part[k].1.push(path);
            }
        }
        let mut placed = FastMap::default();
        if scope == Scope::This {
            self.count_placed(self.root, &mut placed);
        }
        for (part, paths) in by_part {
            // Every placement painted: the part itself, whatever the scope.
            let whole = scope == Scope::All || placed.get(&part).copied().unwrap_or(0) == paths.len();
            self.paint_groups(part, Some(layer), &[(material, paths)], whole.then_some(0))?;
        }
        Ok(())
    }

    /// Paints the part `part` itself on `layer`: its definition, whatever its occurrences.
    pub(crate) fn paint_part(&mut self, part: Id, layer: Family, material: Option<Id>) -> Result<()> {
        self.part(part)?;
        self.paint_groups(part, Some(layer), &[(material, Vec::new())], Some(0))
    }

    /// Gives the parts of a document whose occurrences carried items of the catalog (a SketchUp
    /// file's instances, docs/io/sketchup-import.md) those items, each where its family goes
    /// (a material, a finish): `painted` by (assembly, occurrence), a part taking the
    /// nearest one above it. A part reached in several keeps the
    /// one most of its placements take; the others get a copy of it each.
    pub(crate) fn settle_occurrence_paints(&mut self, painted: &FastMap<(Id, Id), Id>) -> Result<()> {
        if painted.is_empty() {
            return Ok(());
        }
        let mut by_part: Vec<(Id, Groups)> = Vec::new();
        let mut at: FastMap<Id, usize> = FastMap::default();
        let mut stack: Vec<(Id, Vec<Id>, Option<Id>)> = vec![(self.root, Vec::new(), None)];
        while let Some((assembly, path, above)) = stack.pop() {
            let Ok(a) = self.assembly(assembly) else { continue };
            for o in a.occurrences() {
                let material = painted.get(&(assembly, o.id)).copied().or(above);
                if self.part(o.component).is_ok() {
                    let k = *at.entry(o.component).or_insert_with(|| {
                        by_part.push((o.component, Vec::new()));
                        by_part.len() - 1
                    });
                    let groups = &mut by_part[k].1;
                    let here = EntityPath::new(path.clone(), o.id);
                    match groups.iter_mut().find(|(m, _)| *m == material) {
                        Some((_, g)) => g.push(here),
                        None => groups.push((material, vec![here])),
                    }
                } else if path.len() < crate::MAX_DEPTH {
                    let mut below = path.clone();
                    below.push(o.id);
                    stack.push((o.component, below, material));
                }
            }
        }
        for (part, groups) in by_part {
            // The largest group (the first of the largest) keeps the part.
            let kept = (0..groups.len()).rev().max_by_key(|&g| groups[g].1.len());
            self.paint_groups(part, None, &groups, kept)?;
        }
        Ok(())
    }

    /// Paints `layer` (none: each group's material's family, a group without one left as it is) of
    /// the placements `groups` of `part`, each group in its material: the group `kept`
    /// on `part` itself, every other on a copy of its own. An occurrence all of whose
    /// placements are in a group is pointed at its copy where it stands; one placed in several
    /// groups (its assembly shown elsewhere) has the assemblies on the way made their own first,
    /// as « only this one » does.
    fn paint_groups(
        &mut self,
        part: Id,
        layer: Option<Family>,
        groups: &[(Option<Id>, Vec<EntityPath>)],
        kept: Option<usize>,
    ) -> Result<()> {
        // Every copy made first, from the part as it was: one group's paint is not another's.
        let mut owns = Vec::with_capacity(groups.len());
        for (g, (_, paths)) in groups.iter().enumerate() {
            owns.push(if Some(g) == kept {
                part
            } else {
                let copy = self.copy_component(part)?;
                // How many times each assembly is placed, as the tree now stands.
                let mut placed = FastMap::default();
                self.count_placed(self.root, &mut placed);
                placed.insert(self.root, 1);
                // The placements by occurrence: its assembly, its id.
                let mut slots: Vec<((Id, Id), Vec<&EntityPath>)> = Vec::new();
                for p in paths {
                    let slot = (self.context_of(&p.occurrences)?, p.entity);
                    match slots.iter_mut().find(|(s, _)| *s == slot) {
                        Some((_, list)) => list.push(p),
                        None => slots.push((slot, vec![p])),
                    }
                }
                for ((assembly, entity), list) in slots {
                    if placed.get(&assembly).copied().unwrap_or(0) == list.len() {
                        if let Some(o) = self.assembly_mut(assembly)?.occurrence_mut(entity) {
                            o.component = copy;
                        }
                    } else {
                        for p in list {
                            self.separate_onto(p, copy)?;
                        }
                    }
                }
                copy
            });
        }
        for ((material, _), own) in groups.iter().zip(owns) {
            let Some(layer) = layer.or_else(|| material.and_then(|m| self.material(m).ok()).map(|m| m.family())) else { continue };
            let now = match layer {
                Family::Material => self.part(own)?.material,
                _ => self.part(own)?.finish,
            };
            if now != *material
                && let Some(slot) = self.part_mut(own)?.slot_mut(layer)
            {
                *slot = *material;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Coat, compose};
    use crate::BlendMode;

    #[test]
    fn coats_compose_into_one_map_of_what_is_under() {
        let coat = |blend, intensity, c: f32| Coat { blend, intensity, color: [c, c, c] };
        let apply = |(a, b): ([f32; 3], [f32; 3]), x: f32| x * a[0] + b[0];
        // A paint covering fully hides what is under; half covering, half of each.
        assert!((apply(compose(&[coat(BlendMode::Cover, 1.0, 0.2)]), 0.9) - 0.2).abs() < 1e-6);
        assert!((apply(compose(&[coat(BlendMode::Cover, 0.5, 0.2)]), 0.8) - 0.5).abs() < 1e-6);
        // A stain multiplies: the wood darker, its grain (x) still there.
        assert!((apply(compose(&[coat(BlendMode::Tint, 1.0, 0.5)]), 0.8) - 0.4).abs() < 1e-6);
        assert!((apply(compose(&[coat(BlendMode::Tint, 0.5, 0.5)]), 0.8) - 0.6).abs() < 1e-6);
        // A sequence: a stain then a covering varnish at 20 %, one map, as laid one after the other.
        let two = compose(&[coat(BlendMode::Tint, 1.0, 0.5), coat(BlendMode::Cover, 0.2, 1.0)]);
        assert!((apply(two, 0.8) - (0.8 * 0.5 * 0.8 + 0.2)).abs() < 1e-6);
    }
}

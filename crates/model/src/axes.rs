//! Changing a component's axes (docs/decisions/0053-changing-a-components-axes.md): nothing moves
//! in the world, what is stored in those axes is brought into the new ones.

use glam::{DAffine3, DMat3, DVec3};

use crate::document::{Behavior, ComponentKind, Document, Id, StretchMask, is_identity};
use crate::drawing::{Direction, ElementKind, Frame, Look, Perspective, Subject, View, ViewKind};
use crate::ops::{Solver, is_rigid};
use crate::{Anchor, DimKind, Error, Refusal, Result};

const STANDARD: [Direction; 7] =
    [Direction::Front, Direction::Back, Direction::Left, Direction::Right, Direction::Top, Direction::Bottom, Direction::Iso];

impl Document {
    /// Gives `component` new axes, `axes` going from them to the current ones: a direct rigid
    /// transform. Every occurrence stays where it stands.
    pub(crate) fn change_axes(&mut self, component: Id, axes: &DAffine3, solver: &mut dyn Solver) -> Result<()> {
        if component == self.root {
            return Err(Error::Invalid(Refusal::RootAxes));
        }
        if !is_rigid(axes) || axes.matrix3.determinant() <= 0.0 {
            return Err(Error::Invalid(Refusal::AxesNotDirect));
        }
        self.component(component)?;
        if is_identity(axes) {
            return Ok(());
        }
        let into = axes.inverse();
        let users: Vec<Id> = self
            .components
            .iter()
            .filter(|(_, c)| c.as_assembly().is_some_and(|a| a.occurrences().iter().any(|o| o.component == component)))
            .map(|(id, _)| *id)
            .collect();
        for id in users {
            let a = self.assembly_mut(id)?;
            for i in 0..a.occurrences().len() {
                if a.occurrences()[i].component == component {
                    let o = a.occurrence_at_mut(i);
                    o.transform *= *axes;
                }
            }
        }
        let c = self.component_mut(component)?;
        for g in &mut c.guides {
            g.shape = g.shape.transformed(&into);
        }
        c.behavior = turned_behavior(&c.behavior, &axes.matrix3);
        match &mut c.kind {
            ComponentKind::Part(p) => {
                let shape = p.shape().clone();
                for b in &mut p.bodies {
                    b.placement = settled(into * b.placement);
                }
                // Brought back to its body's own axes, it is that body again.
                if p.is_plain() {
                    p.shape = None;
                } else if !p.bodies.is_empty() {
                    p.shape = Some(solver.transform(&shape, &into)?);
                }
            }
            ComponentKind::Assembly(a) => {
                for i in 0..a.occurrences().len() {
                    let o = a.occurrence_at_mut(i);
                    o.transform = settled(into * o.transform);
                }
            }
        }
        let drawing = std::sync::Arc::make_mut(&mut self.drawing);
        for list in drawing.lists_mut() {
            for e in list.iter_mut() {
                match &mut e.kind {
                    ElementKind::View(View { kind: ViewKind::Base { subject: Subject::Component(c), look, .. }, .. })
                        if *c == component =>
                    {
                        *look = turned_look(look, &into);
                    }
                    ElementKind::Dimension(d) => {
                        let anchor = |a: &mut Anchor| {
                            if a.part == component {
                                a.at = into.transform_point3(a.at);
                            }
                        };
                        match &mut d.kind {
                            DimKind::Linear { a, b, .. } | DimKind::Angle { a, b, .. } => {
                                anchor(a);
                                anchor(b);
                            }
                            DimKind::Radius { edge, .. } => anchor(edge),
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

/// `t`, exactly the identity when it is one to rounding: a part back in its body's axes is plain.
fn settled(t: DAffine3) -> DAffine3 {
    if is_identity(&t) { DAffine3::IDENTITY } else { t }
}

/// For each new axis, the current one it lies on and whether it is reversed; none for an oblique
/// rotation.
fn quarter_turn(m: &DMat3) -> Option<[(usize, bool); 3]> {
    let on = |v: DVec3| {
        let k = v.abs().max_position();
        ((v.abs()[k] - 1.0).abs() < 1e-9).then_some((k, v[k] < 0.0))
    };
    Some([on(m.x_axis)?, on(m.y_axis)?, on(m.z_axis)?])
}

/// The stretch settings in the new axes, `m` their directions in the current ones: permuted on a
/// quarter turn, the default on an oblique rotation, where they mean nothing.
fn turned_behavior(b: &Behavior, m: &DMat3) -> Behavior {
    let Some(turn) = quarter_turn(m) else {
        return Behavior { no_stretch: StretchMask(b.no_stretch.0 & !0b111), cutters: None };
    };
    let mut no_stretch = StretchMask(b.no_stretch.0 & !0b111);
    for (k, (from, _)) in turn.iter().enumerate() {
        no_stretch = no_stretch.with(k, b.no_stretch.locks(*from));
    }
    let cutters = b.cutters.as_ref().map(|c| {
        turn.map(|(from, reversed)| match reversed {
            true => c[from].iter().rev().map(|r| 1.0 - r).collect(),
            false => c[from].clone(),
        })
    });
    Behavior { no_stretch, cutters }
}

/// The same point of view in the new axes, `into` going to them. The subject turns with it, so
/// its box seen from there, and the view's place and crop on the sheet, stay as they are.
fn turned_look(look: &Look, into: &DAffine3) -> Look {
    let (frame, perspective) = look.frame();
    let (forward, up) = (into.transform_vector3(frame.forward), into.transform_vector3(frame.up));
    if matches!(look, Look::Standard(_)) {
        let turned = Frame::looking(forward, up);
        let same = |a: DVec3, b: DVec3| (a - b).length() < 1e-9;
        let standard = STANDARD.into_iter().find(|d| {
            let f = Frame::of(*d);
            same(f.forward, turned.forward) && same(f.up, turned.up)
        });
        if let Some(d) = standard {
            return Look::Standard(d);
        }
    }
    let perspective = perspective.map(|p| Perspective { eye: into.transform_point3(p.eye), ..p });
    Look::Camera { forward, up, perspective }
}

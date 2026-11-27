//! Changing a component's axes (docs/decisions/0054-changing-a-components-axes.md): nothing moves
//! in the world, what is stored in those axes is brought into the new ones.

use glam::{DAffine3, DMat3, DVec3};
use skraft_kernel::orient;

use crate::document::{Behavior, ComponentKind, Document, Id, StretchMask, is_identity};
use crate::drawing::{Direction, ElementKind, Frame, Look, Perspective, Subject, View, ViewKind};
use crate::ops::{Solver, is_rigid};
use crate::{Anchor, DimKind, Error, Primitive, Refusal, Result};
use skraft_kernel::profile;

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
        // Linked bodies moved with the rest: their seats are in these axes too. The bodies it
        // imprints elsewhere are in its axes: they stay where they are, in the new ones.
        if self.part(component).is_ok() {
            self.map_seats(component, None, &into);
            self.map_source_axes(component, axes, solver)?;
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

/// A primitive as a tool draws it, its part born oriented.
pub(crate) struct Born {
    /// What to build.
    pub primitive: Primitive,
    /// Placed by this, before the drawing's placement.
    pub placed: DAffine3,
    /// The axes still to give the part made: a shape whose own axes cannot be its part's.
    pub axes: Option<DAffine3>,
}

/// `primitive` for a part oriented by OpenCutList's rule ([`drawn_axes`]). A box or a prism whose
/// blue axis is its height is turned in its plane, built in those axes and stays a plain part, at
/// no cost, its origin at its low corner as any part drawn; the others are given new axes once
/// made, about their origin, so that their profile stays the outline drawn (a tall box's
/// rectangle, as a tall prism's or cylinder's).
pub(crate) fn oriented(primitive: &Primitive) -> Born {
    let (local, at) = primitive.at_origin();
    let here = DAffine3::from_translation(at);
    let Some(m) = drawn_axes(&local).filter(|m| !m.abs_diff_eq(DMat3::IDENTITY, 1e-12)) else {
        return Born { primitive: primitive.clone(), placed: DAffine3::IDENTITY, axes: None };
    };
    let a = DAffine3::from_mat3(m);
    let into = a.inverse();
    let flat = m.z_axis.abs_diff_eq(DVec3::Z, 1e-12);
    match &local {
        Primitive::Box { size, .. } if flat => {
            let turned = into.transform_vector3(*size);
            Born { primitive: Primitive::Box { origin: turned.min(DVec3::ZERO), size: turned.abs() }, placed: here * a, axes: None }
        }
        Primitive::Prism { profile, height, .. } if flat => {
            let corners = profile.iter().map(|c| profile::Corner { p: into.transform_point3(c.p.extend(0.0)).truncate(), ..*c }).collect();
            Born { primitive: Primitive::Prism { origin: DVec3::ZERO, profile: corners, height: *height }, placed: here * a, axes: None }
        }
        _ => Born { primitive: local, placed: here, axes: Some(a) },
    }
}

/// The axes OpenCutList gives a shape it draws, in the primitive's, from its parameters (no
/// kernel): on its largest faces and longest edges; a round shape's length along its height only
/// when it exceeds its diameter. None for a shape with no straight edge to go by.
fn drawn_axes(p: &Primitive) -> Option<DMat3> {
    let corners = |(lo, hi): (DVec3, DVec3)| -> Vec<DVec3> {
        (0..8).map(|k| DVec3::new([lo.x, hi.x][k & 1], [lo.y, hi.y][(k >> 1) & 1], [lo.z, hi.z][(k >> 2) & 1])).collect()
    };
    match p {
        Primitive::Box { size: s, .. } => {
            let faces = [(DVec3::Z, 2.0 * s.x * s.y), (DVec3::Y, 2.0 * s.x * s.z), (DVec3::X, 2.0 * s.y * s.z)];
            let edges = [(DVec3::X, 4.0 * s.x), (DVec3::Y, 4.0 * s.y), (DVec3::Z, 4.0 * s.z)];
            orient::axes_on(&faces, &edges, &corners(primitive_box(p)?))
        }
        Primitive::Prism { profile: outline, height, .. } => {
            let round = profile::rounded(outline);
            let h = height.abs();
            let mut faces = vec![(DVec3::Z, profile::signed_area2(&round).abs())];
            let mut edges = Vec::new();
            for side in profile::sides(&round) {
                if let profile::Side::Line { a, b } = side {
                    let d = (b - a).normalize_or_zero();
                    faces.push((DVec3::new(d.y, -d.x, 0.0), a.distance(b) * h));
                    edges.push((d.extend(0.0), 2.0 * a.distance(b)));
                }
            }
            edges.push((DVec3::Z, h * round.len() as f64));
            let points: Vec<DVec3> = profile::sample(&round, 0.2).into_iter().flat_map(|q| [q.extend(0.0), q.extend(*height)]).collect();
            orient::axes_on(&faces, &edges, &points)
        }
        Primitive::Cylinder { .. } | Primitive::Cone { .. } | Primitive::Revolution { .. } => {
            let (lo, hi) = primitive_box(p)?;
            let (x, z) = if hi.z - lo.z > hi.x - lo.x { (DVec3::Z, DVec3::X) } else { (DVec3::X, DVec3::Z) };
            Some(orient::signed_axes(&corners((lo, hi)), x, z))
        }
        Primitive::Sphere { .. } | Primitive::Torus { .. } => None,
    }
}

/// The box of a primitive at its origin, from its parameters: no kernel.
fn primitive_box(p: &Primitive) -> Option<(DVec3, DVec3)> {
    let round = |r: f64, lo: f64, hi: f64| (DVec3::new(-r, -r, lo.min(hi)), DVec3::new(r, r, lo.max(hi)));
    Some(match p {
        Primitive::Box { origin, size } => ((*origin).min(*origin + *size), (*origin).max(*origin + *size)),
        Primitive::Cylinder { radius, height, .. } => round(*radius, 0.0, *height),
        Primitive::Sphere { radius, .. } => round(*radius, -*radius, *radius),
        Primitive::Cone { base_radius, top_radius, height, .. } => round(base_radius.max(*top_radius), 0.0, *height),
        Primitive::Torus { major_radius, minor_radius, .. } => round(major_radius + minor_radius, -*minor_radius, *minor_radius),
        Primitive::Prism { profile, height, .. } => {
            let (lo, hi) = profile::bounds(&profile::rounded(profile))?;
            (lo.extend(height.min(0.0)), hi.extend(height.max(0.0)))
        }
        Primitive::Revolution { profile, .. } => {
            let (lo, hi) = profile::bounds(&profile::rounded(profile))?;
            round(hi.x, lo.y, hi.y)
        }
    })
}

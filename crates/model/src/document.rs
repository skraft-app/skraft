//! The document: components — parts and assemblies — placed by occurrences
//! (docs/model/data-model.md).
//!
//! The document is immutable piece by piece: components and solids sit behind `Arc`, so a
//! clone shares everything and an operation copies only what it changes (`Arc::make_mut`).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

use glam::{DAffine3, DVec3};
use serde::{Deserialize, Serialize};

use crate::props::{PropDef, Props};
use skraft_i18n::t;

use crate::settings::Settings;
use skraft_kernel::Solid;

use crate::{Error, FastMap, FastSet, Operation, Result};

/// A stable identifier, from one counter: unique across a document for components, within
/// their assembly for occurrences and within their part for bodies (a copy of a component
/// keeps them, docs/model/data-model.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(pub u64);

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// A shared, named definition: a part or an assembly (docs/model/data-model.md).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Component {
    pub id: Id,
    pub name: String,
    /// Named by the user (else its name is automatic, docs/model/bodies.md#automatic-names). Named
    /// or not, a component stays when unused, in the library's « Inutilisés », until purged
    /// (docs/ui/components-library.md).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub named: bool,
    pub kind: ComponentKind,
    /// Construction guides, in the component's axes (docs/tools/tape-measure.md).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guides: Vec<Guide>,
    /// How it is stretched: its cutters and locked axes (docs/tools/stretch.md).
    #[serde(default, skip_serializing_if = "Behavior::is_default")]
    pub behavior: Behavior,
    /// Its typed properties (docs/model/properties.md).
    #[serde(default, skip_serializing_if = "Props::is_empty")]
    pub props: Props,
}

/// How a component behaves when stretched (docs/tools/stretch.md, after SketchUp's
/// `Behavior`): the axes it never stretches along, and where its cutters stand. Both follow
/// the component: its copies keep them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Behavior {
    /// The axes (its own) it never stretches along.
    #[serde(default, skip_serializing_if = "StretchMask::is_empty")]
    pub no_stretch: StretchMask,
    /// The cutters along each of its axes, as increasing ratios of its box in ]0, 1[; none:
    /// one in the middle. A locked axis has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cutters: Option<[Vec<f64>; 3]>,
}

impl Behavior {
    pub fn is_default(&self) -> bool {
        *self == Behavior::default()
    }

    /// A known mask, and cutters finite, inside ]0, 1[ and increasing.
    pub fn is_valid(&self) -> bool {
        let increasing = |r: &Vec<f64>| r.iter().all(|x| x.is_finite() && *x > 0.0 && *x < 1.0) && r.windows(2).all(|w| w[0] < w[1]);
        self.no_stretch.0 <= StretchMask::ALL && self.cutters.as_ref().is_none_or(|c| c.iter().all(increasing))
    }

    /// The same, the cutters of its locked axes dropped (the mask is the one truth).
    pub fn normalized(mut self) -> Behavior {
        if let Some(c) = &mut self.cutters {
            for (k, r) in c.iter_mut().enumerate() {
                if self.no_stretch.locks(k) {
                    r.clear();
                }
            }
        }
        self
    }
}

/// The axes a component never stretches along, in its own axes: SketchUp's `no_scale_mask`
/// bits — X 1, Y 2, Z 4; the planes' and the uniform scale's (8 to 64) kept, unused (there is
/// no scaling: stretching is ours).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StretchMask(pub u8);

impl StretchMask {
    /// Every bit SketchUp knows.
    pub const ALL: u8 = 0b111_1111;

    pub fn is_empty(&self) -> bool {
        self.0 & 0b111 == 0
    }

    /// Is the axis `k` (0 X, 1 Y, 2 Z) locked?
    pub fn locks(self, k: usize) -> bool {
        k < 3 && self.0 & (1 << k) != 0
    }

    /// The same, the axis `k` locked or not.
    pub fn with(self, k: usize, locked: bool) -> StretchMask {
        if locked { StretchMask(self.0 | (1 << k)) } else { StretchMask(self.0 & !(1 << k)) }
    }

    /// Does it lock a stretch along `axis` (unit, in the component's axes)? As soon as the
    /// axis has a component along a locked one.
    pub fn locks_along(self, axis: DVec3) -> bool {
        (0..3).any(|k| self.locks(k) && axis[k].abs() > 1e-9)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentKind {
    Part(Part),
    Assembly(Assembly),
}

impl Component {
    pub fn as_part(&self) -> Option<&Part> {
        match &self.kind {
            ComponentKind::Part(p) => Some(p),
            ComponentKind::Assembly(_) => None,
        }
    }

    pub fn as_assembly(&self) -> Option<&Assembly> {
        match &self.kind {
            ComponentKind::Assembly(a) => Some(a),
            ComponentKind::Part(_) => None,
        }
    }

    pub fn is_part(&self) -> bool {
        self.as_part().is_some()
    }

    pub fn guide(&self, id: Id) -> Option<&Guide> {
        self.guides.iter().find(|g| g.id == id)
    }
}

/// A construction guide of a component (docs/tools/tape-measure.md): drawn and inferred, never cut.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Guide {
    pub id: Id,
    pub shape: GuideShape,
    /// Its typed properties (docs/model/properties.md).
    #[serde(default, skip_serializing_if = "Props::is_empty")]
    pub props: Props,
}

impl Guide {
    /// A copy of it, `id`, its shape moved by `t`: its properties go along.
    pub fn moved(&self, id: Id, t: &DAffine3) -> Guide {
        Guide { id, shape: self.shape.transformed(t), props: self.props.clone() }
    }
}

/// A guide's geometry, in its component's axes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuideShape {
    /// The infinite line through `origin` along `dir` (unit).
    Line { origin: DVec3, dir: DVec3 },
    /// The whole circle of `radius` about `center`, in the plane of `normal` (unit).
    Circle { center: DVec3, normal: DVec3, radius: f64 },
}

impl GuideShape {
    /// The shape moved by the rigid `t`, its directions kept unit.
    pub fn transformed(&self, t: &DAffine3) -> GuideShape {
        match *self {
            GuideShape::Line { origin, dir } => {
                GuideShape::Line { origin: t.transform_point3(origin), dir: t.transform_vector3(dir).normalize_or_zero() }
            }
            GuideShape::Circle { center, normal, radius } => {
                GuideShape::Circle { center: t.transform_point3(center), normal: t.transform_vector3(normal).normalize_or_zero(), radius }
            }
        }
    }

    /// Finite values, unit directions, a positive radius.
    pub fn is_valid(&self) -> bool {
        let unit = |v: DVec3| v.is_finite() && (v.length() - 1.0).abs() < 1e-6;
        match *self {
            GuideShape::Line { origin, dir } => origin.is_finite() && unit(dir),
            GuideShape::Circle { center, normal, radius } => center.is_finite() && unit(normal) && radius.is_finite() && radius > 0.0,
        }
    }
}

/// A part: the union of its positive bodies minus the union of its negative ones, every body
/// placed in the part's own axes (docs/model/data-model.md, docs/model/data-model.md, docs/model/bodies.md). A body is an
/// occurrence of one of the part's body definitions: copies share theirs, a change to its shape
/// shows in every one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Part {
    /// The shapes of the bodies, each used by one body at least.
    pub defs: Vec<BodyDef>,
    pub bodies: Vec<Body>,
    /// The part as last computed from its bodies; absent when it is its single positive body,
    /// unplaced (no kernel). A cache saved with the file: the bodies are the source.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "crate::file::solid_opt")]
    pub(crate) shape: Option<Arc<Solid>>,
    /// The bodies whose boolean failed when the shape was computed: skipped, the others
    /// computed (docs/model/data-model.md). Part of the same cache.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub(crate) failed: BTreeSet<Id>,
    /// What the part is made of, a material of a part's type (docs/render/look.md): counted,
    /// and seen under its coloration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<Id>,
    /// What the part looks like, a coloration, over its material.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coloration: Option<Id>,
    /// The veneers and edges of single faces of the shape, by their index in its record. A
    /// boolean renumbers faces: an index may then name another face, or none (ignored).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub coverings: BTreeMap<u32, Id>,
    /// The colorations of single faces, by their index as [`Part::coverings`]: over the face's
    /// covering, else over the part.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub faces: BTreeMap<u32, Id>,
}

impl Part {
    /// A part of the single positive body `solid`, unplaced: its definition `def`, its body
    /// `id`.
    pub fn new(def: Id, id: Id, solid: Arc<Solid>) -> Self {
        Part::of_body(BodyDef::new(def, solid), Body::new(id, def, Sign::Positive, DAffine3::IDENTITY))
    }

    /// A part of the single positive body `body`, of `def`; computed when it is placed.
    pub(crate) fn of_body(def: BodyDef, body: Body) -> Self {
        Part {
            defs: vec![def],
            bodies: vec![body],
            shape: None,
            failed: BTreeSet::new(),
            material: None,
            coloration: None,
            coverings: BTreeMap::new(),
            faces: BTreeMap::new(),
        }
    }

    /// Settles the automatic names of its body definitions (docs/model/bodies.md#body-names): each its
    /// nature's, numbered from #1, unique in the part — one of another nature (an extrusion
    /// stretched askew is a free solid now), or taken by an earlier definition (a copy made
    /// unique), or none yet, numbered anew. Whether one changed.
    pub(crate) fn settle_names(&mut self) -> bool {
        if self.names_settled() {
            return false;
        }
        let mut taken: crate::FastSet<String> = self.defs.iter().filter(|d| d.named).map(|d| d.name.clone()).collect();
        let mut renamed = vec![false; self.defs.len()];
        for (i, d) in self.defs.iter().enumerate() {
            if !d.named && !(name_base(&d.name) == d.nature().base() && d.name.contains('#') && taken.insert(d.name.clone())) {
                renamed[i] = true;
            }
        }
        for (d, renamed) in self.defs.iter_mut().zip(renamed) {
            if renamed {
                d.name = numbered_name(&d.nature().base(), 1, |n| taken.contains(n));
                taken.insert(d.name.clone());
            }
        }
        true
    }

    /// Whether its automatic names are settled ([`Part::settle_names`]), without allocating
    /// for the usual few definitions (many: not known, settled again).
    fn names_settled(&self) -> bool {
        self.defs.len() <= 32
            && self.defs.iter().enumerate().all(|(i, d)| {
                d.named
                    || (d.name.contains('#')
                        && name_base(&d.name) == d.nature().base()
                        && !self.defs[..i].iter().any(|e| e.name == d.name)
                        && !self.defs[i + 1..].iter().any(|e| e.named && e.name == d.name))
            })
    }

    /// The part as it is drawn and used: its bodies combined.
    pub fn shape(&self) -> &Arc<Solid> {
        match &self.shape {
            Some(s) => s,
            // The single positive body is the shape (`is_plain`).
            None => self.solid_of(&self.bodies[0]),
        }
    }

    pub fn def(&self, id: Id) -> Option<&BodyDef> {
        self.defs.iter().find(|d| d.id == id)
    }

    /// The definition of `b`, a body of this part (a part read or changed keeps every body's).
    pub fn def_of(&self, b: &Body) -> &BodyDef {
        self.def(b.def).expect("a body's definition is in its part")
    }

    /// The solid of `b`, in its own axes.
    pub fn solid_of(&self, b: &Body) -> &Arc<Solid> {
        &self.def_of(b).solid
    }

    /// The profile `b`'s solid is built from, if any.
    pub fn profile_of(&self, b: &Body) -> Option<&crate::BodyProfile> {
        self.def_of(b).profile.as_ref()
    }

    /// How many bodies use the definition `def`, those of groups included (a group's children
    /// counted once, however many copies of the group there are).
    pub fn copies_of(&self, def: Id) -> usize {
        self.all_bodies().filter(|b| b.def == def).count()
    }

    /// Every body of the part: its own, then those of the groups they lead to, at any depth
    /// (each group's once, however many copies of it there are).
    pub fn all_bodies(&self) -> impl Iterator<Item = &Body> {
        let used = used_defs(&self.defs, &self.bodies);
        self.bodies.iter().chain(self.defs.iter().filter(move |d| used.contains(&d.id)).flat_map(|d| d.children()))
    }

    /// The bodies of the group `def` uses (none for a leaf or an unknown definition).
    pub fn children_of(&self, def: Id) -> &[Body] {
        self.def(def).map_or(&[], |d| d.children())
    }

    /// The group definitions the bodies `group` lead through (the part's own body first, each
    /// a group's, the next one among its children), and the last one's placement in the part's
    /// axes.
    pub fn group_chain(&self, group: &[Id]) -> crate::Result<(Vec<Id>, DAffine3)> {
        let mut chain = Vec::with_capacity(group.len());
        let mut placed = DAffine3::IDENTITY;
        let mut bodies: &[Body] = &self.bodies;
        for id in group {
            let b = bodies.iter().find(|b| b.id == *id).ok_or(Error::NotFound(*id))?;
            let d = self.def(b.def).filter(|d| d.is_group()).ok_or(Error::NotFound(*id))?;
            placed *= b.placement;
            chain.push(d.id);
            bodies = d.children();
        }
        Ok((chain, placed))
    }

    /// The group `def` (one of this part's) seen as a part: its children as the bodies, its
    /// result as the shape, every definition of the part.
    pub(crate) fn group_as_part(&self, def: &BodyDef) -> Part {
        let g = def.group.clone().unwrap_or_default();
        let mut p = Part {
            defs: self.defs.clone(),
            bodies: g.children,
            shape: None,
            failed: g.failed,
            material: self.material,
            coloration: self.coloration,
            coverings: BTreeMap::new(),
            faces: BTreeMap::new(),
        };
        // Only a plain part reads its shape from its body: an emptied group has none to read.
        if !p.is_plain() {
            p.shape = Some(def.solid.clone());
        }
        p
    }

    /// Drops the definitions no body uses, through groups.
    pub(crate) fn drop_unused_defs(&mut self) {
        let used = used_defs(&self.defs, &self.bodies);
        self.defs.retain(|d| used.contains(&d.id));
    }

    /// Is the part its single positive body, unplaced: its own shape, with no kernel?
    pub fn is_plain(&self) -> bool {
        matches!(&self.bodies[..], [b] if b.sign == Sign::Positive && is_identity(&b.placement))
    }

    /// Is the part its single positive body, placed or not: its shape that body's, its faces
    /// in the same order (docs/decisions/0054-changing-a-components-axes.md)?
    /// What the part shows where its faces have nothing of their own: its coloration over its
    /// material (docs/render/look.md#palette-and-materials).
    pub fn shown(&self) -> Seen {
        Seen { under: self.material, over: self.coloration }
    }

    /// What each face with something of its own (a covering, a coloration) shows: its
    /// coloration, else the part's, over its covering, else the part's material.
    pub fn faces_shown(&self) -> BTreeMap<u32, Seen> {
        let faces = self.coverings.keys().chain(self.faces.keys());
        faces
            .map(|f| {
                let under = self.coverings.get(f).copied().or(self.material);
                (*f, Seen { under, over: self.faces.get(f).copied().or(self.coloration) })
            })
            .collect()
    }

    /// Every material and coloration it holds, each slot's (a material may come twice).
    pub fn materials_held(&self) -> impl Iterator<Item = Id> + '_ {
        self.material.into_iter().chain(self.coloration).chain(self.coverings.values().copied()).chain(self.faces.values().copied())
    }

    /// The slot of `layer`, on the part.
    pub(crate) fn slot_mut(&mut self, layer: Family) -> Option<&mut Option<Id>> {
        match layer {
            Family::Material => Some(&mut self.material),
            Family::Coloration => Some(&mut self.coloration),
            Family::Covering => None,
        }
    }

    /// The faces' slots of `layer`.
    pub(crate) fn faces_mut(&mut self, layer: Family) -> Option<&mut BTreeMap<u32, Id>> {
        match layer {
            Family::Covering => Some(&mut self.coverings),
            Family::Coloration => Some(&mut self.faces),
            Family::Material => None,
        }
    }

    pub fn is_one_body(&self) -> bool {
        matches!(&self.bodies[..], [b] if b.sign == Sign::Positive)
    }

    pub fn body(&self, id: Id) -> Option<&Body> {
        self.bodies.iter().find(|b| b.id == id)
    }

    /// The body `id`, the part's own or a group's (ids are unique in the whole part).
    pub fn any_body(&self, id: Id) -> Option<&Body> {
        let groups = self.defs.iter().filter_map(|d| d.group.as_ref()).flat_map(|g| g.children.iter());
        self.bodies.iter().chain(groups).find(|b| b.id == id)
    }

    /// The body `id`, the part's own or a group's, to change.
    pub(crate) fn any_body_mut(&mut self, id: Id) -> Option<&mut Body> {
        let groups = self.defs.iter_mut().filter_map(|d| d.group.as_mut()).flat_map(|g| g.children.iter_mut());
        self.bodies.iter_mut().chain(groups).find(|b| b.id == id)
    }

    pub fn positives(&self) -> impl Iterator<Item = &Body> {
        self.bodies.iter().filter(|b| b.sign == Sign::Positive)
    }

    pub fn negatives(&self) -> impl Iterator<Item = &Body> {
        self.bodies.iter().filter(|b| b.sign == Sign::Negative)
    }

    /// Its intersections (∩ bodies), each limiting it (docs/decisions/0045-intersection-a-third-sign.md).
    pub fn intersects(&self) -> impl Iterator<Item = &Body> {
        self.bodies.iter().filter(|b| b.sign == Sign::Intersect)
    }

    /// The first positive body with a profile: the one the Profile tool edits for now.
    pub fn profiled(&self) -> Option<&Body> {
        self.positives().find(|b| self.profile_of(b).is_some())
    }

    /// Did the body `id` fail, the last time the shape (or its group) was computed?
    pub fn is_failed(&self, id: Id) -> bool {
        self.failed.contains(&id)
            || self.defs.iter().any(|d| d.group.as_ref().is_some_and(|g| g.failed.contains(&id)))
            || self.refusal_of(id).is_some()
    }

    /// Why the body node `id` (at any depth) left its children's result as it was, when it did
    /// (docs/decisions/0042-body-nodes-operators.md).
    pub fn refusal_of(&self, id: Id) -> Option<&skraft_kernel::names::NodeRefusal> {
        let body = self.bodies.iter().chain(self.defs.iter().flat_map(|d| d.children())).find(|b| b.id == id)?;
        self.def(body.def)?.group.as_ref()?.refused.as_ref()
    }

    /// Is the body `b` computed as it should be, all it holds too (nothing failed, no node
    /// refused)?
    pub(crate) fn is_sound(&self, b: &Body) -> bool {
        fn sound(p: &Part, b: &Body, depth: usize) -> bool {
            !p.is_failed(b.id) && depth <= crate::MAX_DEPTH && p.def_of(b).children().iter().all(|c| sound(p, c, depth + 1))
        }
        sound(self, b, 0)
    }

    /// Does it hold more than one plain body: several, or a group or node?
    pub fn has_tree(&self) -> bool {
        self.bodies.len() > 1 || self.bodies.iter().any(|b| self.def_of(b).is_group())
    }

    /// How many separate lumps of material the shape holds: a part is one piece, more
    /// is an error the user has to correct (docs/model/bodies.md).
    pub fn pieces(&self) -> usize {
        self.shape().pieces()
    }
}

/// The definitions `bodies` use, through groups (`defs` the part's).
pub(crate) fn used_defs(defs: &[BodyDef], bodies: &[Body]) -> BTreeSet<Id> {
    let mut used = BTreeSet::new();
    let mut stack: Vec<Id> = bodies.iter().map(|b| b.def).collect();
    while let Some(id) = stack.pop() {
        if used.insert(id)
            && let Some(d) = defs.iter().find(|d| d.id == id)
        {
            stack.extend(d.children().iter().map(|b| b.def));
        }
    }
    used
}

/// `t` within a hair of the identity.
pub(crate) fn is_identity(t: &DAffine3) -> bool {
    t.abs_diff_eq(DAffine3::IDENTITY, 1e-12)
}

/// Whether a body adds material to its part, takes it away, or keeps only what lies inside it (∩, an intersection: what lies outside
/// it is taken away, docs/decisions/0045-intersection-a-third-sign.md) (docs/model/bodies.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sign {
    Positive,
    Negative,
    Intersect,
}

/// The shape of bodies of a part (docs/model/bodies.md): `solid`, in its own axes — a body's. A leaf holds its
/// shape (a free one, or built from its profile); a group (docs/model/bodies.md#body-groups) holds bodies,
/// `solid` then only their result cached: `(∪ positive children) − (∪ negative children)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BodyDef {
    pub id: Id,
    /// Its name (docs/model/bodies.md#body-names), every body of it showing it: given by the user
    /// (`named`), kept when its shape changes (a stretch, a boolean, a new profile) and by its
    /// copies; else automatic, its nature's numbered among its part's (« Extrusion#2 »),
    /// settled after each operation ([`Part::settle_names`]).
    #[serde(default)]
    pub name: String,
    /// The name given by the user, not automatic.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub named: bool,
    #[serde(with = "crate::file::solid")]
    pub solid: Arc<Solid>,
    /// The profile `solid` is built from (docs/tools/prism.md), in its axes: the solid is then its
    /// cache. None for a free shape (imported, a boolean's result, stretched askew) and a group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<crate::BodyProfile>,
    #[serde(default)]
    pub source: BodySource,
    /// The bodies a group is made of, placed in its axes; none for a leaf.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<BodyGroup>,
    /// A free shape flattened from a group (docs/model/bodies.md#flattening): the names its
    /// faces had, for a node above to find its edges again; held only for the solid whose
    /// fingerprint is `kept_faces_of` ([`BodyDef::face_map`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kept_faces: skraft_kernel::names::FaceMap,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub kept_faces_of: u64,
    /// A linked body made by « Empreindre », from its source's final solid
    /// (docs/decisions/0055-linked-bodies-through-imprints.md): its solid is then a cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imprint: Option<crate::ImprintRecipe>,
    /// Its typed properties (docs/model/properties.md).
    #[serde(default, skip_serializing_if = "Props::is_empty")]
    pub props: Props,
}

impl BodyDef {
    /// A free shape (no profile), local.
    pub fn new(id: Id, solid: Arc<Solid>) -> Self {
        BodyDef {
            id,
            name: String::new(),
            named: false,
            solid,
            profile: None,
            source: BodySource::Local,
            group: None,
            kept_faces: Vec::new(),
            kept_faces_of: 0,
            imprint: None,
            props: Props::default(),
        }
    }

    /// A group of `children`, `solid` their result, `failed` those whose boolean failed.
    pub fn group(id: Id, solid: Arc<Solid>, children: Vec<Body>, failed: BTreeSet<Id>) -> Self {
        BodyDef { group: Some(BodyGroup::plain(children, failed)), ..BodyDef::new(id, solid) }
    }

    /// The bodies of a group, none for a leaf.
    pub fn children(&self) -> &[Body] {
        self.group.as_ref().map_or(&[], |g| &g.children)
    }

    pub fn is_group(&self) -> bool {
        self.group.is_some()
    }

    /// Its group's face map, or those a flattened group kept, when it is that of its solid as
    /// it is (docs/decisions/0042-body-nodes-operators.md).
    pub fn face_map(&self) -> Option<&skraft_kernel::names::FaceMap> {
        let (faces, of) = self.group.as_ref().map_or((&self.kept_faces, self.kept_faces_of), |g| (&g.faces, g.faces_of));
        (!faces.is_empty() && of == skraft_kernel::names::fingerprint(self.solid.record())).then_some(faces)
    }

    /// Its node's operation, when it is a body node (docs/decisions/0042-body-nodes-operators.md).
    pub fn op(&self) -> Option<&skraft_kernel::names::NodeOp> {
        self.group.as_ref().and_then(|g| g.op.as_ref())
    }

    /// What it is, which its automatic name and its icon tell.
    pub fn nature(&self) -> Nature {
        use skraft_kernel::names::{Blend, NodeOp};
        match (self.op(), &self.profile) {
            (Some(NodeOp::Blend(b)), _) => match b.kind {
                Blend::Round => Nature::Round,
                Blend::Chamfer => Nature::Chamfer,
            },
            _ if self.is_group() => Nature::Group,
            (_, Some(f)) if f.is_turned() => Nature::Revolution,
            (_, Some(_)) => Nature::Extrusion,
            (_, None) => Nature::Solid,
        }
    }

    /// Takes the name `name` from the user, or (`None`) an automatic one again.
    pub fn set_name(&mut self, name: Option<String>) {
        (self.name, self.named) = match name {
            Some(n) => (n, true),
            None => (String::new(), false),
        };
    }
}

/// What a body definition is (docs/model/bodies.md#body-names): a group, a body of a profile extruded or
/// turned, else a free solid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Nature {
    Extrusion,
    Revolution,
    Solid,
    Group,
    /// A body node rounding edges of its children's result.
    Round,
    /// A body node chamfering them.
    Chamfer,
}

impl Nature {
    /// The base of its automatic names: « Extrusion » of « Extrusion#2 ».
    pub fn base(self) -> String {
        match self {
            Nature::Extrusion => t!("default_name.extrusion"),
            Nature::Revolution => t!("default_name.revolution"),
            Nature::Solid => t!("default_name.solid"),
            Nature::Group => t!("default_name.group"),
            Nature::Round => t!("default_name.round"),
            Nature::Chamfer => t!("default_name.chamfer"),
        }
    }
}

/// The base of a name numbered as automatic names are: `base` of `base#3`, else the name.
pub fn name_base(name: &str) -> &str {
    match name.rsplit_once('#') {
        Some((base, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => base,
        _ => name,
    }
}

/// The first of `base#first`, `base#first+1`, … not `taken`.
pub(crate) fn numbered_name(base: &str, first: u32, taken: impl Fn(&str) -> bool) -> String {
    (first..).map(|i| format!("{base}#{i}")).find(|n| !taken(n)).unwrap_or_default()
}

/// The content of a group of bodies (docs/model/bodies.md#body-groups): its bodies, each an occurrence of a
/// definition of the same part, placed in the group's axes. Its negatives cut its positives
/// only. Body ids are unique in the whole part, groups included.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BodyGroup {
    pub children: Vec<Body>,
    /// The children whose boolean failed when the group was computed (skipped).
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub failed: BTreeSet<Id>,
    /// What a body node does to its children's result (a round, a chamfer, of edges named in
    /// it: docs/decisions/0042-body-nodes-operators.md); none for a plain group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op: Option<skraft_kernel::names::NodeOp>,
    /// Why the node last left its children's result as it was (its names lost, its size too
    /// large): the node is in error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refused: Option<skraft_kernel::names::NodeRefusal>,
    /// Its result's faces' names (relative to it), for a node above it not to compute it again
    /// from its leaves (docs/decisions/0042-body-nodes-operators.md); held only for the solid
    /// whose fingerprint is `faces_of` ([`BodyDef::face_map`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: skraft_kernel::names::FaceMap,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub faces_of: u64,
    /// A body node's children's result as last computed, in memory only: what the node starts
    /// from next time its children are the same (its size or edges changed, a preview) or only
    /// cutters were added (docs/model/bodies.md#body-nodes).
    #[serde(skip)]
    pub raw: RawCell,
}

/// A body node's children's result, before its operation, and its face map (relative to the
/// node), for the children `tree` whose leaves are `leaves` (the same `Arc`s: held, so their
/// addresses stay theirs).
#[derive(Debug)]
pub struct RawCache {
    pub tree: Vec<skraft_kernel::names::TreeBody>,
    pub leaves: Vec<Arc<Solid>>,
    pub solid: Arc<Solid>,
    pub faces: skraft_kernel::names::FaceMap,
}

impl RawCache {
    /// Does it hold for the children `tree` of leaves `leaves`?
    pub fn holds(&self, tree: &[skraft_kernel::names::TreeBody], leaves: &[Arc<Solid>]) -> bool {
        self.tree == tree && self.leaves.len() == leaves.len() && self.leaves.iter().zip(leaves).all(|(a, b)| Arc::ptr_eq(a, b))
    }

    /// The children of `tree` (leaves `leaves`) added after those it holds for, when they are
    /// all it lacks (the first ones as it holds them, the leaves too) and all cutters or intersections:
    /// what the node can cut or intersect it with, rather than computing its children again.
    pub fn cutters<'a>(
        &self,
        tree: &'a [skraft_kernel::names::TreeBody],
        leaves: &[Arc<Solid>],
    ) -> Option<&'a [skraft_kernel::names::TreeBody]> {
        let n = self.tree.len();
        let added = tree.get(n..).filter(|a| !a.is_empty())?;
        (tree[..n] == self.tree[..]
            && leaves.len() >= self.leaves.len()
            && self.leaves.iter().zip(leaves).all(|(a, b)| Arc::ptr_eq(a, b))
            && added.iter().all(|b| b.negative || b.intersect))
        .then_some(added)
    }
}

/// A [`RawCache`], or none: a cache, which a document's equality ignores.
#[derive(Clone, Debug, Default)]
pub struct RawCell(pub Option<Arc<RawCache>>);

impl PartialEq for RawCell {
    fn eq(&self, _: &RawCell) -> bool {
        true
    }
}

fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}

impl BodyGroup {
    /// A plain group of `children`, `failed` those whose boolean failed.
    pub fn plain(children: Vec<Body>, failed: BTreeSet<Id>) -> BodyGroup {
        BodyGroup { children, failed, op: None, refused: None, faces: Vec::new(), faces_of: 0, raw: RawCell::default() }
    }

    /// The face map `faces` of the solid `solid` (none: forgotten).
    pub fn with_faces(self, solid: &Solid, faces: Option<skraft_kernel::names::FaceMap>) -> BodyGroup {
        match faces {
            Some(faces) if faces.len() == solid.record().faces.len() && !faces.is_empty() => {
                BodyGroup { faces, faces_of: skraft_kernel::names::fingerprint(solid.record()), ..self }
            }
            _ => BodyGroup { faces: Vec::new(), faces_of: 0, ..self },
        }
    }
}

/// A solid of a part: an occurrence of the definition `def`, placed by `placement` in the part's
/// axes, adding material or taking it away (`sign`; a negative body is a machining, docs/model/data-model.md).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Body {
    pub id: Id,
    /// Its own name, as an occurrence's (« Haut <Trou charnière#1> », docs/model/bodies.md#body-names):
    /// empty for none; kept by its copies.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub def: Id,
    pub sign: Sign,
    pub placement: DAffine3,
    /// Its typed properties (docs/model/properties.md).
    #[serde(default, skip_serializing_if = "Props::is_empty")]
    pub props: Props,
}

impl Body {
    pub fn new(id: Id, def: Id, sign: Sign, placement: DAffine3) -> Self {
        Body { id, name: String::new(), def, sign, placement, props: Props::default() }
    }
}

/// Where a body's shape comes from: its own, or the imprint of another part, its solid then a
/// cache (docs/decisions/0055-linked-bodies-through-imprints.md).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodySource {
    #[default]
    Local,
    Linked,
}

/// An assembly: occurrences of other components.
///
/// Found by id through an index (id → position), dropped by any change to the list and built
/// again only once searched a few times since (a copy made to change one occurrence is not
/// indexed for a search or two; a clone keeps the index): a search per occurrence of a large
/// assembly stays linear in all, not quadratic. An occurrence's id never changes.
#[derive(Default, Serialize, Deserialize)]
pub struct Assembly {
    occurrences: Vec<Occurrence>,
    /// The links between occurrences it holds, at any depth
    /// (docs/decisions/0055-linked-bodies-through-imprints.md).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    links: Vec<crate::Link>,
    #[serde(skip)]
    index: OnceLock<FastMap<Id, u32>>,
    /// Searches by scanning since the index was dropped.
    #[serde(skip)]
    scans: AtomicU32,
}

/// Below this many occurrences, a search scans them (cheaper than hashing).
const INDEXED_FROM: usize = 16;
/// Searches by scanning before the index is built.
const SCANS_BEFORE_INDEX: u32 = 4;

impl Assembly {
    pub fn new(occurrences: Vec<Occurrence>) -> Self {
        Assembly { occurrences, links: Vec::new(), index: OnceLock::new(), scans: AtomicU32::new(0) }
    }

    pub fn occurrences(&self) -> &[Occurrence] {
        &self.occurrences
    }

    /// The links it holds (docs/decisions/0055-linked-bodies-through-imprints.md).
    pub fn links(&self) -> &[crate::Link] {
        &self.links
    }

    pub(crate) fn links_mut(&mut self) -> &mut Vec<crate::Link> {
        &mut self.links
    }

    /// The list, to change: the index is dropped.
    pub(crate) fn occurrences_mut(&mut self) -> &mut Vec<Occurrence> {
        self.index = OnceLock::new();
        self.scans = AtomicU32::new(0);
        &mut self.occurrences
    }

    pub fn occurrence(&self, id: Id) -> Option<&Occurrence> {
        self.position(id).map(|i| &self.occurrences[i])
    }

    /// The occurrence at `i` (its [`Assembly::position`]), to change but for its id: the
    /// index stays.
    pub(crate) fn occurrence_at_mut(&mut self, i: usize) -> &mut Occurrence {
        &mut self.occurrences[i]
    }

    pub(crate) fn occurrence_mut(&mut self, id: Id) -> Option<&mut Occurrence> {
        let i = self.position(id)?;
        Some(&mut self.occurrences[i])
    }

    pub(crate) fn position(&self, id: Id) -> Option<usize> {
        let scan = || self.occurrences.iter().position(|o| o.id == id);
        if self.occurrences.len() < INDEXED_FROM {
            return scan();
        }
        if self.index.get().is_none() && self.scans.fetch_add(1, Ordering::Relaxed) < SCANS_BEFORE_INDEX {
            return scan();
        }
        let index = self.index.get_or_init(|| self.occurrences.iter().enumerate().map(|(i, o)| (o.id, i as u32)).collect());
        index.get(&id).map(|i| *i as usize).filter(|i| self.occurrences[*i].id == id)
    }
}

impl Clone for Assembly {
    fn clone(&self) -> Self {
        Assembly {
            occurrences: self.occurrences.clone(),
            links: self.links.clone(),
            index: self.index.clone(),
            scans: AtomicU32::new(self.scans.load(Ordering::Relaxed)),
        }
    }
}

impl PartialEq for Assembly {
    fn eq(&self, other: &Self) -> bool {
        self.occurrences == other.occurrences && self.links == other.links
    }
}

impl std::fmt::Debug for Assembly {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Assembly").field("occurrences", &self.occurrences).field("links", &self.links).finish()
    }
}

/// A component placed: one copy of a part or an assembly (docs/model/data-model.md).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Occurrence {
    pub id: Id,
    /// The copy's own name ("Gauche", "Droite"); may be empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub component: Id,
    /// From the component's axes to its assembly's: a move, a rotation, maybe a mirror; never
    /// a scale.
    pub transform: DAffine3,
    /// Hidden by the user (docs/tools/interaction.md): neither drawn, picked, inferred on nor framed, nor is
    /// anything under it — but while it is being edited, or with the hidden shown.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// Its typed properties (docs/model/properties.md).
    #[serde(default, skip_serializing_if = "Props::is_empty")]
    pub props: Props,
}

/// The three families of the « Matières » card, each a table of the document
/// (docs/decisions/0057-materials-on-the-definition-typed-sources.md), and where each goes on a
/// part.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    /// What a part is made of, counted and cut ([`Part::material`]).
    #[default]
    Material,
    /// What covers a face: a veneer, an edge banding ([`Part::coverings`]).
    Covering,
    /// What colours a part or a face, never counted ([`Part::coloration`], [`Part::faces`]).
    Coloration,
}

impl Family {
    pub const ALL: [Family; 3] = [Family::Material, Family::Covering, Family::Coloration];

    /// Its types, in the order they are offered, the first a new item's (none for colorations).
    pub fn kinds(self) -> &'static [MaterialKind] {
        match self {
            Family::Material => &[MaterialKind::Solid, MaterialKind::Sheet, MaterialKind::Profile],
            Family::Covering => &[MaterialKind::Veneer, MaterialKind::Edge],
            Family::Coloration => &[MaterialKind::Coat, MaterialKind::Scheme],
        }
    }
}

/// A material's or a covering's type: how it is counted and cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialKind {
    /// Solid wood, a block of metal… (« Massif »).
    Solid,
    /// A sheet good (« Panneau »).
    Sheet,
    /// Dimensional timber, a tube, a bar (« Profilé »).
    Profile,
    /// A veneer (« Placage »).
    Veneer,
    /// An edge banding (« Chant »).
    Edge,
    /// A coloration of its own: a colour, a mode, an intensity (« Couche »).
    Coat,
    /// A coloration made of coats of the catalog, in order (« Schéma »).
    Scheme,
}

/// How a coat lays its colour over what is under it (docs/decisions/0057-materials-on-the-definition-typed-sources.md).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    /// Laid over it, its intensity its opacity (« Couvrant »): a paint.
    #[default]
    Cover,
    /// Multiplying it, its intensity its strength (« Teintant »): a stain, a tinted varnish.
    Tint,
}

impl BlendMode {
    fn is_cover(&self) -> bool {
        *self == BlendMode::Cover
    }
}

fn is_full(x: &f32) -> bool {
    *x == 1.0
}

fn full() -> f32 {
    1.0
}

impl MaterialKind {
    /// The family it is a type of.
    pub fn family(self) -> Family {
        match self {
            MaterialKind::Solid | MaterialKind::Sheet | MaterialKind::Profile => Family::Material,
            MaterialKind::Veneer | MaterialKind::Edge => Family::Covering,
            MaterialKind::Coat | MaterialKind::Scheme => Family::Coloration,
        }
    }
}

/// An item of the catalog — a material, a covering or a coloration: a name, a colour, maybe a
/// texture (docs/render/look.md), its type, which tells its family ([`Catalog`]); a coat's
/// blend and intensity, a scheme's coats.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<MaterialKind>,
    /// sRGB; under a texture, what is shown until its image is loaded.
    pub color: [u8; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<Texture>,
    /// A coat's: how it lays its colour over what is under it.
    #[serde(default, skip_serializing_if = "BlendMode::is_cover")]
    pub blend: BlendMode,
    /// A coat's: its opacity, or its strength when it tints, from 0 to 1.
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub intensity: f32,
    /// A scheme's: the coats it lays, from the bottom up, referred to (a coat may come twice).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub coats: Vec<Id>,
    /// Its typed properties (docs/model/properties.md).
    #[serde(default, skip_serializing_if = "Props::is_empty")]
    pub props: Props,
}

impl Material {
    /// A coat of `color`, named `name`, covering, without texture nor properties.
    pub fn new(name: String, color: [u8; 3]) -> Self {
        Material::of_kind(name, color, None)
    }

    /// An item of `kind` (none: a coat) and `color`, named `name`.
    pub fn of_kind(name: String, color: [u8; 3], kind: Option<MaterialKind>) -> Self {
        let kind = Some(kind.unwrap_or(MaterialKind::Coat));
        Material { name, kind, color, texture: None, blend: BlendMode::Cover, intensity: 1.0, coats: Vec::new(), props: Props::default() }
    }

    /// Its family: its type's (a coloration without one).
    pub fn family(&self) -> Family {
        self.kind.map_or(Family::Coloration, MaterialKind::family)
    }

    pub fn is_scheme(&self) -> bool {
        self.kind == Some(MaterialKind::Scheme)
    }
}

/// The document's catalog: a table per family (docs/decisions/0057-materials-on-the-definition-typed-sources.md),
/// the ids unique across them (all from `next_id`). Each item's family is told by its type
/// ([`Material::family`]), set right when a file is read: a table's item takes its family's
/// first type when it has none of its family.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "CatalogFile", into = "CatalogFile")]
pub struct Catalog {
    materials: BTreeMap<Id, Material>,
    coverings: BTreeMap<Id, Material>,
    colorations: BTreeMap<Id, Material>,
}

/// The catalog as a file holds it: three tables.
#[derive(Clone, Default, Serialize, Deserialize)]
struct CatalogFile {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    materials: BTreeMap<Id, Material>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    coverings: BTreeMap<Id, Material>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    colorations: BTreeMap<Id, Material>,
}

impl From<CatalogFile> for Catalog {
    fn from(f: CatalogFile) -> Self {
        let mut c = Catalog::default();
        for (family, table) in [(Family::Material, f.materials), (Family::Covering, f.coverings), (Family::Coloration, f.colorations)] {
            for (id, mut m) in table {
                if m.family() != family || m.kind.is_none() {
                    m.kind = family.kinds().first().copied();
                }
                c.insert(id, m);
            }
        }
        c
    }
}

impl From<Catalog> for CatalogFile {
    fn from(c: Catalog) -> Self {
        CatalogFile { materials: c.materials, coverings: c.coverings, colorations: c.colorations }
    }
}

impl FromIterator<(Id, Material)> for Catalog {
    fn from_iter<I: IntoIterator<Item = (Id, Material)>>(items: I) -> Self {
        let mut c = Catalog::default();
        for (id, m) in items {
            c.insert(id, m);
        }
        c
    }
}

impl Catalog {
    fn table(&self, family: Family) -> &BTreeMap<Id, Material> {
        match family {
            Family::Material => &self.materials,
            Family::Covering => &self.coverings,
            Family::Coloration => &self.colorations,
        }
    }

    fn table_mut(&mut self, family: Family) -> &mut BTreeMap<Id, Material> {
        match family {
            Family::Material => &mut self.materials,
            Family::Covering => &mut self.coverings,
            Family::Coloration => &mut self.colorations,
        }
    }

    /// Adds `m` to its family's table.
    pub(crate) fn insert(&mut self, id: Id, m: Material) {
        self.table_mut(m.family()).insert(id, m);
    }

    pub(crate) fn get(&self, id: &Id) -> Option<&Material> {
        Family::ALL.iter().find_map(|f| self.table(*f).get(id))
    }

    pub(crate) fn get_mut(&mut self, id: &Id) -> Option<&mut Material> {
        let family = Family::ALL.into_iter().find(|f| self.table(*f).contains_key(id))?;
        self.table_mut(family).get_mut(id)
    }

    pub(crate) fn contains_key(&self, id: &Id) -> bool {
        self.get(id).is_some()
    }

    pub(crate) fn remove(&mut self, id: &Id) -> Option<Material> {
        Family::ALL.into_iter().find_map(|f| self.table_mut(f).remove(id))
    }

    /// Every item, family by family, each in id order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&Id, &Material)> {
        self.materials.iter().chain(&self.coverings).chain(&self.colorations)
    }

    pub(crate) fn keys(&self) -> impl Iterator<Item = &Id> {
        self.iter().map(|(id, _)| id)
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &Material> {
        self.iter().map(|(_, m)| m)
    }

    pub(crate) fn values_mut(&mut self) -> impl Iterator<Item = &mut Material> {
        self.materials.values_mut().chain(self.coverings.values_mut()).chain(self.colorations.values_mut())
    }

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&Id, &mut Material) -> bool) {
        for f in Family::ALL {
            self.table_mut(f).retain(&mut keep);
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.materials.len() + self.coverings.len() + self.colorations.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The items of `family`, in id order.
    pub fn family(&self, family: Family) -> impl Iterator<Item = (Id, &Material)> {
        self.table(family).iter().map(|(id, m)| (*id, m))
    }
}

/// A material's texture (docs/render/look.md#textures): an image of the document's assets, laid
/// on the faces at its real size by a projection in the part's axes, its width along the
/// part's length (a wood's grain).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Texture {
    /// Its image: its id among the document's assets ([`crate::Asset::id`]).
    pub image: String,
    /// Its real size (mm): the image's width, along the grain, then its height.
    pub size: [f64; 2],
    /// Turned in the face's plane (radians).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rotation: f64,
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

impl Texture {
    /// Well formed: a size positive and finite, a rotation finite.
    pub fn is_valid(&self) -> bool {
        self.size.iter().all(|s| s.is_finite() && *s > 0.0) && self.rotation.is_finite()
    }
}

/// A guide in the scene (docs/tools/tape-measure.md): the occurrences from the root to the
/// component it belongs to — an assembly, or a part placed by the last one —, then its id. Two
/// occurrences of one component make two paths to the same guide.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GuidePath {
    /// Occurrence ids from the root, outermost first; empty for a guide of the model.
    pub occurrences: Vec<Id>,
    pub guide: Id,
}

impl GuidePath {
    pub fn new(occurrences: Vec<Id>, guide: Id) -> Self {
        GuidePath { occurrences, guide }
    }

    pub fn root(guide: Id) -> Self {
        GuidePath::new(Vec::new(), guide)
    }
}

/// An occurrence in the scene: the occurrences of assemblies leading to it from the root,
/// then the occurrence itself. Two occurrences of one assembly make two paths to the same part.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntityPath {
    /// Occurrence ids from the root, outermost first; empty for an occurrence of the root.
    pub occurrences: Vec<Id>,
    pub entity: Id,
    /// In the part `entity` places, the group of bodies open (docs/model/bodies.md#body-groups): the
    /// bodies leading to it, the part's own first, each a group's. Empty: the part itself. Only
    /// the operations on bodies, and [`Document::resolve_container`], read it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub group: Vec<Id>,
}

impl EntityPath {
    pub fn new(occurrences: Vec<Id>, entity: Id) -> Self {
        EntityPath { occurrences, entity, group: Vec::new() }
    }

    pub fn root(entity: Id) -> Self {
        EntityPath::new(Vec::new(), entity)
    }

    /// This path into the group of bodies `group` of its part.
    pub fn in_group(mut self, group: Vec<Id>) -> Self {
        self.group = group;
        self
    }

    /// This path to the part itself, out of any group.
    pub fn part_path(&self) -> EntityPath {
        EntityPath::new(self.occurrences.clone(), self.entity)
    }

    /// The path `chain` (occurrences from the root, the last one included) names.
    pub fn from_chain(chain: &[Id]) -> Option<Self> {
        let (last, rest) = chain.split_last()?;
        Some(EntityPath::new(rest.to_vec(), *last))
    }
}

/// [`Document::placed_changes`]'s walk: what it compares with, and what it found.
struct ChangesWalk<'a> {
    before: &'a Document,
    touched: &'a [Id],
    /// By component: does it differ from `before`'s, itself or below?
    dirty: FastMap<Id, bool>,
    out: Vec<(PlacedPart, Id, DAffine3)>,
}

/// A part placed in the scene, by [`Document::placed_parts`].
#[derive(Clone, Debug)]
pub struct PlacedPart {
    pub path: EntityPath,
    pub part: Id,
    /// Part to world.
    pub transform: DAffine3,
    pub shape: Arc<Solid>,
    /// The part's material ([`Part::material`]), what is counted.
    pub material: Option<Id>,
    /// The part's coloration ([`Part::coloration`]).
    pub coloration: Option<Id>,
    /// The deepest hidden occurrence of its path, by its index in the path (the occurrence
    /// itself last): none when nothing along it is hidden.
    pub hidden_at: Option<u32>,
}

impl PlacedPart {
    /// What it shows where its faces have nothing of their own ([`Part::shown`]).
    pub fn shown(&self) -> Seen {
        Seen { under: self.material, over: self.coloration }
    }
}

/// What a part or a face shows (docs/render/look.md#palette-and-materials): what is under — a
/// covering or a material, else the theme's default — and the coloration over it, if any.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Seen {
    pub under: Option<Id>,
    pub over: Option<Id>,
}

impl Seen {
    /// Nothing: the theme's default.
    pub const NONE: Seen = Seen { under: None, over: None };

    pub fn is_none(&self) -> bool {
        *self == Seen::NONE
    }

    /// The item named by `id`, as what is seen: a coloration over nothing, else what is under.
    pub fn of(doc: &Document, id: Option<Id>) -> Seen {
        match id.and_then(|m| doc.material(m).ok().map(|x| (m, x.family()))) {
            Some((m, Family::Coloration)) => Seen { under: None, over: Some(m) },
            Some((m, _)) => Seen { under: Some(m), over: None },
            None => Seen::NONE,
        }
    }

    /// The item on top: the coloration, else what is under.
    pub fn top(&self) -> Option<Id> {
        self.over.or(self.under)
    }

    /// The items it names.
    pub fn ids(&self) -> impl Iterator<Item = Id> {
        self.under.into_iter().chain(self.over)
    }
}

/// A guide placed in the scene, by [`Document::placed_guides`].
#[derive(Clone, Debug)]
pub struct PlacedGuide {
    /// The occurrences from the root to the component holding it (empty: the root's).
    pub path: Vec<Id>,
    pub component: Id,
    /// The component's axes to world.
    pub transform: DAffine3,
    pub guide: Guide,
    /// The deepest hidden occurrence of `path`, by its index there: none when nothing along
    /// it is hidden.
    pub hidden_at: Option<u32>,
}

/// The whole model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub(crate) components: BTreeMap<Id, Arc<Component>>,
    /// The root assembly: the model itself, never placed nor counted.
    pub(crate) root: Id,
    pub(crate) next_id: u64,
    /// Display style and units, saved with the model.
    #[serde(default)]
    pub(crate) settings: Settings,
    /// The materials, coverings and colorations parts are painted with, a table each
    /// (docs/decisions/0057-materials-on-the-definition-typed-sources.md).
    #[serde(default, rename = "catalog", skip_serializing_if = "Catalog::is_empty")]
    pub(crate) materials: Catalog,
    /// The definitions of the properties (docs/model/properties.md), by key: optional, a key
    /// without one keeps its values as they are.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) prop_defs: BTreeMap<String, PropDef>,
    /// The images of the materials' textures, by id ([`crate::Asset::id`]): only those a
    /// material uses ([`Document::prune_assets`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", with = "crate::file::solids::images")]
    pub(crate) assets: BTreeMap<String, crate::Asset>,
    /// The sheets of its drawing (docs/ui/drawing.md): shared by the history's snapshots.
    #[serde(default, skip_serializing_if = "crate::drawing::Drawing::is_empty")]
    pub(crate) drawing: Arc<crate::drawing::Drawing>,
    /// While an operation runs in a group of bodies a node computes (the group itself or one
    /// under it), the part standing for that group: its shape is not computed, the node computes
    /// the group from its children afterwards (docs/model/bodies.md#body-nodes).
    #[serde(skip)]
    pub(crate) deferred: Option<Id>,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    /// An empty model.
    pub fn new() -> Self {
        Self::with_settings(Settings::default())
    }

    /// An empty model with these settings (the app's defaults for new models).
    pub fn with_settings(settings: Settings) -> Self {
        let root = Id(1);
        let kind = ComponentKind::Assembly(Assembly::default());
        let c = Component {
            id: root,
            name: t!("default_name.model"),
            named: true,
            kind,
            guides: Vec::new(),
            behavior: Behavior::default(),
            props: Props::default(),
        };
        Document {
            components: BTreeMap::from([(root, Arc::new(c))]),
            root,
            next_id: 2,
            settings,
            materials: Catalog::default(),
            prop_defs: BTreeMap::new(),
            assets: BTreeMap::new(),
            drawing: Arc::default(),
            deferred: None,
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Every item of the catalog — materials, coverings, colorations —, family by family.
    pub fn materials(&self) -> impl Iterator<Item = (Id, &Material)> {
        self.materials.iter().map(|(id, m)| (*id, m))
    }

    /// The items of `family`, in id order.
    pub fn family(&self, family: Family) -> impl Iterator<Item = (Id, &Material)> {
        self.materials.family(family)
    }

    pub fn material(&self, id: Id) -> Result<&Material> {
        self.materials.get(&id).ok_or(Error::NotFound(id))
    }

    /// The image `id` of the materials' textures.
    pub fn asset(&self, id: &str) -> Option<&crate::Asset> {
        self.assets.get(id)
    }

    pub(crate) fn with_assets_pruned(mut self) -> Self {
        self.prune_assets();
        self
    }

    /// Drops the images no material uses any more.
    pub(crate) fn prune_assets(&mut self) {
        let used: BTreeSet<&str> =
            self.materials.values().filter_map(|m| m.texture.as_ref()).map(|t| t.image.as_str()).chain(self.drawing.images()).collect();
        if self.assets.keys().any(|id| !used.contains(id.as_str())) {
            let used: BTreeSet<String> = used.into_iter().map(str::to_owned).collect();
            self.assets.retain(|id, _| used.contains(id));
        }
    }

    /// A document of the materials `ids` alone (all when empty), without geometry: their
    /// textures' images, every definition of properties (a `.skraftmat`, docs/io/file-format.md).
    pub fn materials_fragment(&self, ids: &[Id]) -> Result<Document> {
        let mut out = Document::new();
        for id in ids {
            self.material(*id)?;
        }
        // The schemes' coats come along.
        let mut wanted: FastSet<Id> = ids.iter().copied().collect();
        wanted.extend(ids.iter().filter_map(|id| self.materials.get(id)).flat_map(|m| m.coats.iter().copied()));
        let mut fresh: HashMap<Id, Id> = HashMap::new();
        for (id, _) in self.materials.iter().filter(|(id, _)| ids.is_empty() || wanted.contains(id)) {
            fresh.insert(*id, out.fresh_id());
        }
        for (id, m) in self.materials.iter() {
            if let Some(k) = fresh.get(id) {
                let mut m = m.clone();
                m.coats = m.coats.iter().filter_map(|c| fresh.get(c).copied()).collect();
                out.materials.insert(*k, m);
            }
        }
        out.prop_defs = self.prop_defs.clone();
        out.assets = self.assets.clone();
        out.prune_assets();
        Ok(out)
    }

    pub fn root(&self) -> Id {
        self.root
    }

    /// The id the next thing made will take. It goes back with an undo: the ids of the steps
    /// undone are handed out again.
    pub fn next_id(&self) -> u64 {
        self.next_id
    }

    pub fn root_assembly(&self) -> &Assembly {
        // The root is always an assembly: `new` makes it, files are checked, no operation
        // changes it.
        self.assembly(self.root).expect("the root is an assembly")
    }

    pub fn component(&self, id: Id) -> Result<&Component> {
        self.components.get(&id).map(|c| &**c).ok_or(Error::NotFound(id))
    }

    pub fn assembly(&self, id: Id) -> Result<&Assembly> {
        self.component(id)?.as_assembly().ok_or(Error::NotAnAssembly(id))
    }

    pub fn part(&self, id: Id) -> Result<&Part> {
        self.component(id)?.as_part().ok_or(Error::NotAPart(id))
    }

    /// Every component but the root, in id order.
    pub fn components(&self) -> impl Iterator<Item = &Component> {
        self.components.values().map(|c| &**c).filter(|c| c.id != self.root)
    }

    /// The operation taking away the occurrences of the empty assemblies (docs/model/data-model.md#components-parts-and-assemblies):
    /// an assembly is empty with neither occurrences nor guides, or when it holds only empty
    /// ones — an assembly emptied empties its parent in turn. The root and `keep` (the active
    /// context, kept while it is edited) are never. One `Delete` per assembly holding some,
    /// in one batch; none when there are none placed.
    pub fn dropping_empty_assemblies(&self, keep: Id) -> Option<Operation> {
        let assemblies: Vec<(&Component, &Assembly)> =
            self.components.values().filter_map(|c| c.as_assembly().map(|a| (&**c, a))).collect();
        let empty = self.empty_assemblies(keep);
        if empty.is_empty() {
            return None;
        }
        let deletes: Vec<Operation> = assemblies
            .iter()
            .filter(|(c, _)| !empty.contains(&c.id))
            .filter_map(|(c, a)| {
                let entities: Vec<Id> = a.occurrences().iter().filter(|o| empty.contains(&o.component)).map(|o| o.id).collect();
                (!entities.is_empty()).then_some(Operation::Delete { context: c.id, entities })
            })
            .collect();
        (!deletes.is_empty()).then_some(Operation::Batch(deletes))
    }

    /// The empty assemblies (docs/model/data-model.md#components-parts-and-assemblies): with neither
    /// occurrences nor guides, or holding only empty ones; never the root nor `keep`.
    fn empty_assemblies(&self, keep: Id) -> BTreeSet<Id> {
        let assemblies: Vec<(&Component, &Assembly)> =
            self.components.values().filter_map(|c| c.as_assembly().map(|a| (&**c, a))).collect();
        let mut empty: BTreeSet<Id> = BTreeSet::new();
        // Up the tree, until no assembly more is found empty.
        loop {
            let before = empty.len();
            for (c, a) in &assemblies {
                if c.id != self.root
                    && c.id != keep
                    && !empty.contains(&c.id)
                    && c.guides.is_empty()
                    && a.occurrences().iter().all(|o| empty.contains(&o.component))
                {
                    empty.insert(c.id);
                }
            }
            if empty.len() == before {
                break;
            }
        }
        empty
    }

    /// Drops the empty assemblies placed nowhere (an empty assembly holds nothing to keep:
    /// docs/model/data-model.md#components-parts-and-assemblies), and so on up: one dropped may leave its
    /// parent unused. Any other component stays when unused, until purged
    /// ([`Operation::PurgeUnused`], docs/ui/components-library.md).
    pub(crate) fn drop_unused_empty_assemblies(&mut self) {
        loop {
            let used: FastSet<Id> =
                self.components.values().filter_map(|c| c.as_assembly()).flat_map(|a| a.occurrences.iter().map(|o| o.component)).collect();
            let gone: Vec<Id> = self.empty_assemblies(self.root).into_iter().filter(|id| !used.contains(id)).collect();
            if gone.is_empty() {
                return;
            }
            for id in gone {
                self.components.remove(&id);
            }
        }
    }

    /// How many occurrences of `component` there are, in all assemblies (not multiplied by
    /// the occurrences of those assemblies).
    pub fn occurrence_count(&self, component: Id) -> usize {
        self.components.values().filter_map(|c| c.as_assembly()).flat_map(|a| &a.occurrences).filter(|o| o.component == component).count()
    }

    /// How many times each component is in the model: its occurrences at any depth under the
    /// root, an assembly's multiplying those below it (the cut list's count,
    /// docs/ui/components-library.md#the-card); the root counts 1. A component placed nowhere
    /// under the root is absent (in no assembly, or only in assemblies themselves absent).
    pub fn counts_in_model(&self) -> FastMap<Id, u64> {
        // The components reached from the root, each after every assembly holding it (reverse
        // post-order of a depth-first walk: the graph has no cycle).
        fn walk(doc: &Document, c: Id, seen: &mut FastSet<Id>, order: &mut Vec<Id>) {
            if !seen.insert(c) {
                return;
            }
            if let Ok(a) = doc.assembly(c) {
                for o in a.occurrences() {
                    walk(doc, o.component, seen, order);
                }
            }
            order.push(c);
        }
        let mut seen = FastSet::default();
        let mut order = Vec::new();
        walk(self, self.root, &mut seen, &mut order);
        let mut counts: FastMap<Id, u64> = FastMap::default();
        counts.insert(self.root, 1);
        for &c in order.iter().rev() {
            let n = counts.get(&c).copied().unwrap_or(0);
            if let Ok(a) = self.assembly(c) {
                for o in a.occurrences() {
                    let k = counts.entry(o.component).or_insert(0);
                    *k = k.saturating_add(n);
                }
            }
        }
        counts
    }

    /// How many placed parts of the model show each material (docs/render/look.md): on one of
    /// their faces, or as the part's own. A part counts once per placement, whatever the
    /// number of its faces painted; hidden ones count, as in [`Document::counts_in_model`].
    /// Only the materials shown are in the map.
    pub fn material_counts(&self) -> FastMap<Id, u64> {
        fn walk(doc: &Document, c: Id, seen: &mut FastSet<Id>, order: &mut Vec<Id>) {
            if !seen.insert(c) {
                return;
            }
            if let Ok(a) = doc.assembly(c) {
                for o in a.occurrences() {
                    walk(doc, o.component, seen, order);
                }
            }
            order.push(c);
        }
        let (mut seen, mut order) = (FastSet::default(), Vec::new());
        walk(self, self.root, &mut seen, &mut order);
        // Each component, after all that hold it: how many times it is reached.
        let mut reached: FastMap<Id, u64> = FastMap::default();
        reached.insert(self.root, 1);
        let mut counts: FastMap<Id, u64> = FastMap::default();
        for &c in order.iter().rev() {
            let Some(n) = reached.remove(&c) else { continue };
            let Ok(component) = self.component(c) else { continue };
            match &component.kind {
                ComponentKind::Assembly(a) => {
                    for o in a.occurrences() {
                        let k = reached.entry(o.component).or_insert(0);
                        *k = k.saturating_add(n);
                    }
                }
                ComponentKind::Part(p) => {
                    // A scheme shows its coats too.
                    let held: Vec<Id> = p.materials_held().collect();
                    let coats = held.iter().filter_map(|id| self.materials.get(id)).flat_map(|m| m.coats.iter().copied());
                    let shown: FastSet<Id> = held.iter().copied().chain(coats).collect();
                    for m in shown {
                        let k = counts.entry(m).or_insert(0);
                        *k = k.saturating_add(n);
                    }
                }
            }
        }
        counts
    }

    /// The materials something refers to: a part or a face of any part, placed in the model or
    /// not, and the coats of the schemes they refer to.
    pub fn materials_referenced(&self) -> FastSet<Id> {
        let mut out = FastSet::default();
        for c in self.components.values() {
            if let ComponentKind::Part(p) = &c.kind {
                out.extend(p.materials_held());
            }
        }
        let coats: Vec<Id> = out.iter().filter_map(|id| self.materials.get(id)).flat_map(|m| m.coats.iter().copied()).collect();
        out.extend(coats);
        out
    }

    /// The coats a coloration lays, from the bottom up: a scheme's (those still there), a coat
    /// itself (docs/decisions/0057-materials-on-the-definition-typed-sources.md).
    pub fn coats_of(&self, coloration: Id) -> Vec<(Id, &Material)> {
        match self.materials.get(&coloration) {
            Some(m) if m.is_scheme() => m.coats.iter().filter_map(|c| self.materials.get(c).map(|m| (*c, m))).collect(),
            Some(m) => vec![(coloration, m)],
            None => Vec::new(),
        }
    }

    /// The schemes laying the coat `coat`.
    pub fn schemes_with(&self, coat: Id) -> Vec<Id> {
        self.materials.family(Family::Coloration).filter(|(_, m)| m.is_scheme() && m.coats.contains(&coat)).map(|(id, _)| id).collect()
    }

    /// Does the occurrence path `occurrences` go through an assembly placed more than once — a
    /// change in the assembly it leads into then reaches other copies (outermost first: a copy
    /// of an assembly above makes those below it shared too)?
    pub fn shared_along(&self, occurrences: &[Id]) -> bool {
        (0..occurrences.len()).any(|k| {
            self.context_of(&occurrences[..k])
                .ok()
                .and_then(|c| self.assembly(c).ok())
                .and_then(|a| a.occurrence(occurrences[k]))
                .is_some_and(|o| self.occurrence_count(o.component) > 1)
        })
    }

    /// The assembly an occurrence path leads into (the root for an empty path).
    pub fn context_of(&self, occurrences: &[Id]) -> Result<Id> {
        let mut at = self.root;
        for &i in occurrences {
            let o = self.assembly(at)?.occurrence(i).ok_or(Error::NotFound(i))?;
            self.assembly(o.component)?;
            at = o.component;
        }
        Ok(at)
    }

    /// The component `occurrences` leads into — an assembly, or the part its last occurrence
    /// places — and its axes to the root's.
    pub fn component_at(&self, occurrences: &[Id]) -> Result<(Id, DAffine3)> {
        let Some((&last, parents)) = occurrences.split_last() else { return Ok((self.root, DAffine3::IDENTITY)) };
        let o = self.assembly(self.context_of(parents)?)?.occurrence(last).ok_or(Error::NotFound(last))?;
        Ok((o.component, self.transform_of(parents)? * o.transform))
    }

    /// The guide `path` names, in its component's axes.
    pub fn resolve_guide(&self, path: &GuidePath) -> Result<&Guide> {
        let (c, _) = self.component_at(&path.occurrences)?;
        self.component(c)?.guide(path.guide).ok_or(Error::NotFound(path.guide))
    }

    /// From the axes of the assembly `occurrences` leads into to the root's.
    pub fn transform_of(&self, occurrences: &[Id]) -> Result<DAffine3> {
        let mut at = self.root;
        let mut t = DAffine3::IDENTITY;
        for &i in occurrences {
            let o = self.assembly(at)?.occurrence(i).ok_or(Error::NotFound(i))?;
            t *= o.transform;
            at = o.component;
        }
        Ok(t)
    }

    /// The occurrence a path names.
    pub fn resolve(&self, path: &EntityPath) -> Result<&Occurrence> {
        let ctx = self.context_of(&path.occurrences)?;
        self.assembly(ctx)?.occurrence(path.entity).ok_or(Error::NotFound(path.entity))
    }

    /// The part a path names, and its part-to-world transform.
    pub fn resolve_part(&self, path: &EntityPath) -> Result<(Id, &Part, DAffine3)> {
        let o = self.resolve(path)?;
        let part = self.part(o.component)?;
        Ok((o.component, part, self.transform_of(&path.occurrences)? * o.transform))
    }

    /// What `path` leads into: the part it places — its id, the part, part to world — or,
    /// when it names a group of bodies (`path.group`), that group seen as a part (docs/model/bodies.md « Body
    /// groups »): its children as the bodies, its result as the shape, every definition of
    /// the part, and group to world.
    pub fn resolve_container(&self, path: &EntityPath) -> Result<(Id, std::borrow::Cow<'_, Part>, DAffine3)> {
        let (id, p, t) = self.resolve_part(path)?;
        if path.group.is_empty() {
            return Ok((id, std::borrow::Cow::Borrowed(p), t));
        }
        let (chain, placed) = p.group_chain(&path.group)?;
        let def = p.def(*chain.last().expect("a group path is not empty")).ok_or(Error::NotFound(path.entity))?;
        Ok((id, std::borrow::Cow::Owned(p.group_as_part(def)), t * placed))
    }

    /// Is the occurrence `id` of the assembly `context` an assembly's (can it be entered)?
    pub fn is_assembly_occurrence(&self, context: Id, id: Id) -> bool {
        self.assembly(context).ok().and_then(|a| a.occurrence(id)).is_some_and(|o| self.assembly(o.component).is_ok())
    }

    /// Every part of the scene, placed: a part of an assembly with three occurrences appears
    /// three times.
    pub fn placed_parts(&self) -> Vec<PlacedPart> {
        let mut out = Vec::new();
        self.walk(self.root, &mut Vec::new(), DAffine3::IDENTITY, None, &mut out, 0);
        out
    }

    /// The parts `self` places otherwise than `before` does at the same path — another part,
    /// the same one changed (its component not `before`'s, or one of `touched`), or elsewhere —
    /// as `self` places them, each with what `before` placed there (part and part-to-world).
    /// Paths `before` lacks are left out. What both share (the same components, unchanged down
    /// the tree, placed alike) is skipped whole: a change to a few occurrences of a large model
    /// costs a pass over the assemblies holding them, not a walk of every placement.
    pub fn placed_changes(&self, before: &Document, touched: &[Id]) -> Vec<(PlacedPart, Id, DAffine3)> {
        let mut walk = ChangesWalk { before, touched, dirty: FastMap::default(), out: Vec::new() };
        let t = DAffine3::IDENTITY;
        self.walk_changes(&mut walk, (self.root, before.root), &mut Vec::new(), (t, t), None, 0);
        walk.out
    }

    /// Does `id` differ from `before`'s, or hold (at any depth) a component that does?
    fn dirty(&self, walk: &mut ChangesWalk<'_>, id: Id, depth: usize) -> bool {
        if let Some(d) = walk.dirty.get(&id) {
            return *d;
        }
        let d = depth > MAX_DEPTH
            || walk.touched.contains(&id)
            || match (self.components.get(&id), walk.before.components.get(&id)) {
                (Some(c), Some(cb)) if Arc::ptr_eq(c, cb) => match &c.kind {
                    ComponentKind::Part(_) => false,
                    ComponentKind::Assembly(a) => a.occurrences.iter().any(|o| self.dirty(walk, o.component, depth + 1)),
                },
                _ => true,
            };
        walk.dirty.insert(id, d);
        d
    }

    fn walk_changes(
        &self,
        walk: &mut ChangesWalk<'_>,
        (assembly, was): (Id, Id),
        path: &mut Vec<Id>,
        (t, tb): (DAffine3, DAffine3),
        hidden_at: Option<u32>,
        depth: usize,
    ) {
        let before = walk.before;
        let (Ok(a), Ok(b)) = (self.assembly(assembly), before.assembly(was)) else { return };
        if depth > MAX_DEPTH {
            return;
        }
        // Occurrences keep their order: matched by place, else (some added or removed) by id.
        let mut by_id: Option<FastMap<Id, &Occurrence>> = None;
        // Placed alike above: an occurrence left as it was is placed alike (no product to
        // work out). Neighbours often share their component: the last one found unchanged.
        let same_above = t == tb;
        let mut unchanged: Option<Id> = None;
        for (i, o) in a.occurrences.iter().enumerate() {
            let ob = match b.occurrences.get(i) {
                Some(ob) if ob.id == o.id => ob,
                _ => match by_id.get_or_insert_with(|| b.occurrences.iter().map(|o| (o.id, o)).collect()).get(&o.id) {
                    Some(ob) => *ob,
                    None => continue,
                },
            };
            let mut clean = |c: Id| {
                if unchanged == Some(c) {
                    return true;
                }
                let clean = !self.dirty(walk, c, depth + 1);
                if clean {
                    unchanged = Some(c);
                }
                clean
            };
            if o.component == ob.component && same_above && o.transform == ob.transform && clean(o.component) {
                continue;
            }
            let (t, tb) = (t * o.transform, tb * ob.transform);
            if o.component == ob.component && t.abs_diff_eq(tb, 1e-9) && clean(o.component) {
                continue;
            }
            let (Ok(c), Ok(cb)) = (self.component(o.component), before.component(ob.component)) else { continue };
            let hidden_at = if o.hidden { Some(depth as u32) } else { hidden_at };
            match (&c.kind, &cb.kind) {
                (ComponentKind::Part(p), ComponentKind::Part(_)) => walk.out.push((
                    PlacedPart {
                        path: EntityPath::new(path.clone(), o.id),
                        part: c.id,
                        transform: t,
                        shape: p.shape().clone(),
                        material: p.material,
                        coloration: p.coloration,
                        hidden_at,
                    },
                    cb.id,
                    tb,
                )),
                (ComponentKind::Assembly(_), ComponentKind::Assembly(_)) => {
                    path.push(o.id);
                    self.walk_changes(walk, (c.id, cb.id), path, (t, tb), hidden_at, depth + 1);
                    path.pop();
                }
                _ => {}
            }
        }
    }

    /// `hidden_at`: the deepest hidden occurrence ([`PlacedPart::hidden_at`]) coming from the
    /// occurrences above.
    pub(crate) fn walk(
        &self,
        assembly: Id,
        path: &mut Vec<Id>,
        t: DAffine3,
        hidden_at: Option<u32>,
        out: &mut Vec<PlacedPart>,
        depth: usize,
    ) {
        let Ok(a) = self.assembly(assembly) else { return };
        if depth > MAX_DEPTH {
            return;
        }
        for o in &a.occurrences {
            let Ok(c) = self.component(o.component) else { continue };
            let t = t * o.transform;
            let hidden_at = if o.hidden { Some(depth as u32) } else { hidden_at };
            match &c.kind {
                ComponentKind::Part(p) => out.push(PlacedPart {
                    path: EntityPath::new(path.clone(), o.id),
                    part: c.id,
                    transform: t,
                    shape: p.shape().clone(),
                    material: p.material,
                    coloration: p.coloration,
                    hidden_at,
                }),
                ComponentKind::Assembly(_) => {
                    path.push(o.id);
                    self.walk(c.id, path, t, hidden_at, out, depth + 1);
                    path.pop();
                }
            }
        }
    }

    /// Every component's guides as the scene shows them: the root's, then each occurrence's
    /// down the tree, a part's in each of its occurrences.
    pub fn placed_guides(&self) -> Vec<PlacedGuide> {
        // Most models have none: no walk of every occurrence for them.
        if self.components.values().all(|c| c.guides.is_empty()) {
            return Vec::new();
        }
        let mut out = Vec::new();
        self.walk_guides(self.root, &mut Vec::new(), DAffine3::IDENTITY, None, &mut out, 0);
        out
    }

    fn walk_guides(
        &self,
        component: Id,
        path: &mut Vec<Id>,
        t: DAffine3,
        hidden_at: Option<u32>,
        out: &mut Vec<PlacedGuide>,
        depth: usize,
    ) {
        let Ok(c) = self.component(component) else { return };
        if depth > MAX_DEPTH {
            return;
        }
        for g in &c.guides {
            out.push(PlacedGuide { path: path.clone(), component, transform: t, guide: g.clone(), hidden_at });
        }
        if let ComponentKind::Assembly(a) = &c.kind {
            for o in &a.occurrences {
                path.push(o.id);
                let hidden_at = if o.hidden { Some(depth as u32) } else { hidden_at };
                self.walk_guides(o.component, path, t * o.transform, hidden_at, out, depth + 1);
                path.pop();
            }
        }
    }

    /// Does `component` contain an occurrence of `target`, at any depth (or is it `target`)?
    pub fn contains_component(&self, component: Id, target: Id) -> bool {
        fn go(doc: &Document, c: Id, target: Id, depth: usize) -> bool {
            if c == target {
                return true;
            }
            if depth > MAX_DEPTH {
                return true; // treat a runaway as a cycle
            }
            doc.assembly(c).is_ok_and(|a| a.occurrences.iter().any(|o| go(doc, o.component, target, depth + 1)))
        }
        go(self, component, target, 0)
    }

    // --- mutation helpers, for operations ---------------------------------------------------

    pub(crate) fn fresh_id(&mut self) -> Id {
        let id = Id(self.next_id);
        self.next_id += 1;
        id
    }

    /// A part of the single positive body `solid`, unplaced, its definition and body named
    /// with fresh ids.
    pub(crate) fn plain_part(&mut self, solid: Arc<Solid>) -> Part {
        let def = self.fresh_id();
        Part::new(def, self.fresh_id(), solid)
    }

    pub(crate) fn component_mut(&mut self, id: Id) -> Result<&mut Component> {
        self.components.get_mut(&id).map(Arc::make_mut).ok_or(Error::NotFound(id))
    }

    pub(crate) fn assembly_mut(&mut self, id: Id) -> Result<&mut Assembly> {
        match &mut self.component_mut(id)?.kind {
            ComponentKind::Assembly(a) => Ok(a),
            ComponentKind::Part(_) => Err(Error::NotAnAssembly(id)),
        }
    }

    pub(crate) fn part_mut(&mut self, id: Id) -> Result<&mut Part> {
        match &mut self.component_mut(id)?.kind {
            ComponentKind::Part(p) => Ok(p),
            ComponentKind::Assembly(_) => Err(Error::NotAPart(id)),
        }
    }

    /// Adds a component, under a name no other component has ([`Document::unique_name`]).
    pub(crate) fn insert_component(&mut self, base: &str, named: bool, kind: ComponentKind) -> Id {
        let id = self.fresh_id();
        let name = self.unique_name(base, named);
        self.components.insert(
            id,
            Arc::new(Component { id, name, named, kind, guides: Vec::new(), behavior: Behavior::default(), props: Props::default() }),
        );
        id
    }

    /// Adds a copy of the component `src` (its recipe or its occurrences, its guides — ids
    /// kept —, its behavior and its properties), under a name of its own.
    pub(crate) fn copy_component(&mut self, src: Id) -> Result<Id> {
        let src = self.component(src)?.clone();
        let copy = self.insert_component(&src.name, src.named, src.kind);
        let c = self.component_mut(copy)?;
        (c.guides, c.behavior, c.props) = (src.guides, src.behavior, src.props);
        Ok(copy)
    }

    /// A name not used by another component, from `base` (its number dropped: a copy of
    /// « Caisson#2 » is « Caisson#3 »): given by the user (`named`), `base` if free, else
    /// `base#2`, `base#3`, …; automatic, always numbered from `base#1`, as SketchUp's (docs/model/data-model.md).
    pub(crate) fn unique_name(&self, base: &str, named: bool) -> String {
        let taken = |n: &str| self.components.values().any(|c| c.name == n);
        if named && !taken(base) {
            return base.to_owned();
        }
        // « Caisson » taken: « Caisson#2 »; « Caisson#2 »: the first number free.
        let numbered = name_base(base);
        numbered_name(numbered, if named && numbered == base { 2 } else { 1 }, taken)
    }

    /// Settles the automatic names of the body definitions ([`Part::settle_names`]) of the
    /// parts changed since `before` (all of them without it: a document read or imported).
    pub(crate) fn settle_names(&mut self, before: Option<&Document>) {
        let parts: Vec<Id> = self
            .components
            .iter()
            .filter(|(id, c)| {
                before.is_none_or(|b| b.components.get(*id).is_none_or(|b| !Arc::ptr_eq(b, c)))
                    && matches!(&c.kind, ComponentKind::Part(p) if !p.names_settled())
            })
            .map(|(id, _)| *id)
            .collect();
        for id in parts {
            if let Some(ComponentKind::Part(p)) = self.components.get_mut(&id).map(|c| &mut Arc::make_mut(c).kind) {
                p.settle_names();
            }
        }
    }

    /// Drops the components no occurrence uses that the user did not name (docs/model/data-model.md), and so on
    /// down: an unnamed assembly dropped may leave its parts unused.
    pub(crate) fn purge(&mut self) {
        loop {
            let mut used = crate::FastSet::default();
            for c in self.components.values() {
                if let ComponentKind::Assembly(a) = &c.kind {
                    used.extend(a.occurrences.iter().map(|o| o.component));
                }
            }
            let unused: Vec<Id> =
                self.components.values().filter(|c| c.id != self.root && !c.named && !used.contains(&c.id)).map(|c| c.id).collect();
            if unused.is_empty() {
                return;
            }
            for id in unused {
                self.components.remove(&id);
            }
        }
    }

    /// Copies the components and materials of `fragment` (another document) into this one,
    /// every id drawn afresh, names made unique; a material equal to one already here (same
    /// name and colour) is that one. Returns the fragment root's occurrences, renumbered too,
    /// for the caller to place.
    ///
    /// With `reuse` (the fragment was copied from this document, whose ids still name the same
    /// things, docs/tools/copy-paste.md), the components and materials still here are used as they are; only
    /// the others are copied.
    pub(crate) fn graft(&mut self, fragment: &Document, reuse: bool) -> Result<(Vec<Occurrence>, BTreeMap<String, String>)> {
        // Its properties' definitions taken in, its keys renamed where they would contradict
        // ours (docs/model/properties.md).
        let renamed = self.adopt_prop_defs(&fragment.prop_defs, fragment.each_props());
        let owned;
        let fragment = match renamed.is_empty() {
            true => fragment,
            false => {
                owned = fragment.with_props_renamed(&renamed);
                &owned
            }
        };
        // Decided before anything is added: a fresh id may be one of the fragment's.
        let here: crate::FastSet<Id> = if reuse {
            fragment.materials.keys().filter(|id| self.materials.contains_key(id)).copied().collect()
        } else {
            crate::FastSet::default()
        };
        // The images first: the same id, the same bytes.
        for (id, a) in &fragment.assets {
            self.assets.entry(id.clone()).or_insert_with(|| a.clone());
        }
        let mut materials: HashMap<Id, Id> = HashMap::new();
        let mut added = Vec::new();
        for (id, m) in fragment.materials.iter() {
            let same = if here.contains(id) { Some(*id) } else { self.materials.iter().find(|(_, here)| *here == m).map(|(k, _)| *k) };
            let to = match same {
                Some(k) => k,
                None => {
                    let k = self.fresh_id();
                    self.materials.insert(k, m.clone());
                    added.push(k);
                    k
                }
            };
            materials.insert(*id, to);
        }
        // The schemes added lay the coats as they came in.
        for k in added {
            if let Some(m) = self.materials.get_mut(&k)
                && !m.coats.is_empty()
            {
                m.coats = m.coats.iter().filter_map(|c| materials.get(c).copied()).collect();
            }
        }
        // Decided before anything is added: a fresh id may be one of the fragment's.
        let kept: crate::FastSet<Id> = if reuse {
            fragment
                .components
                .keys()
                .filter(|id| **id != fragment.root && **id != self.root && self.components.contains_key(id))
                .copied()
                .collect()
        } else {
            crate::FastSet::default()
        };
        let components: HashMap<Id, Id> = fragment
            .components
            .keys()
            .filter(|id| **id != fragment.root)
            .map(|id| (*id, if kept.contains(id) { *id } else { self.fresh_id() }))
            .collect();
        let occurrences = |doc: &mut Document, list: &[Occurrence]| -> Result<Vec<Occurrence>> {
            list.iter()
                .map(|o| {
                    let component = *components.get(&o.component).ok_or(Error::NotFound(o.component))?;
                    Ok(Occurrence {
                        id: doc.fresh_id(),
                        name: o.name.clone(),
                        props: o.props.clone(),
                        component,
                        transform: o.transform,
                        hidden: o.hidden,
                    })
                })
                .collect()
        };
        // In the fragment's order: ids and names come out the same each time.
        for (old, c) in fragment.components.iter().filter(|(id, _)| **id != fragment.root && !kept.contains(*id)) {
            let new = &components[old];
            let kind = match &c.kind {
                ComponentKind::Assembly(a) => ComponentKind::Assembly(Assembly::new(occurrences(self, &a.occurrences)?)),
                ComponentKind::Part(p) => {
                    let mut p = p.clone();
                    // Its body definitions' ids drawn afresh too: the fragment's may be past
                    // this document's counter, or taken by the ids drawn for its bodies.
                    let defs: HashMap<Id, Id> = p.defs.iter().map(|d| (d.id, self.fresh_id())).collect();
                    for d in &mut p.defs {
                        d.id = defs[&d.id];
                        if let Some(g) = &mut d.group {
                            g.children.iter_mut().for_each(|b| b.def = defs.get(&b.def).copied().unwrap_or(b.def));
                        }
                    }
                    let mut bodies: HashMap<Id, Id> = HashMap::new();
                    for b in &mut p.bodies {
                        let id = self.fresh_id();
                        bodies.insert(b.id, id);
                        b.id = id;
                        b.def = defs.get(&b.def).copied().unwrap_or(b.def);
                    }
                    p.failed = p.failed.iter().filter_map(|id| bodies.get(id).copied()).collect();
                    for g in p.defs.iter_mut().filter_map(|d| d.group.as_mut()) {
                        let mut own: HashMap<Id, Id> = HashMap::new();
                        for b in &mut g.children {
                            let id = self.fresh_id();
                            own.insert(b.id, id);
                            b.id = id;
                        }
                        g.failed = g.failed.iter().filter_map(|id| own.get(id).copied()).collect();
                    }
                    p.material = p.material.and_then(|m| materials.get(&m).copied());
                    p.coloration = p.coloration.and_then(|m| materials.get(&m).copied());
                    p.coverings = p.coverings.iter().filter_map(|(f, m)| materials.get(m).map(|m| (*f, *m))).collect();
                    p.faces = p.faces.iter().filter_map(|(f, m)| materials.get(m).map(|m| (*f, *m))).collect();
                    ComponentKind::Part(p)
                }
            };
            let guides = c.guides.iter().map(|g| Guide { id: self.fresh_id(), ..g.clone() }).collect();
            let name = self.unique_name(&c.name, c.named);
            self.components.insert(
                *new,
                Arc::new(Component { id: *new, name, named: c.named, kind, guides, behavior: c.behavior.clone(), props: c.props.clone() }),
            );
        }
        // The links of the assemblies copied, through the ids drawn afresh: their occurrences
        // and bodies stand at the same positions (docs/decisions/0055-linked-bodies-through-imprints.md).
        for (old, c) in fragment.components.iter().filter(|(id, _)| **id != fragment.root && !kept.contains(*id)) {
            let ComponentKind::Assembly(a) = &c.kind else { continue };
            if a.links.is_empty() {
                continue;
            }
            let walk = |doc: &Document, path: &[Id]| -> Option<(Vec<Id>, Id)> {
                let (mut at, mut out) = (*old, Vec::with_capacity(path.len()));
                for &i in path {
                    let from = fragment.assembly(at).ok()?;
                    let k = from.position(i)?;
                    out.push(if kept.contains(&at) { i } else { doc.assembly(components[&at]).ok()?.occurrences[k].id });
                    at = from.occurrences[k].component;
                }
                Some((out, at))
            };
            let body = |doc: &Document, part: Id, id: Id| -> Option<Id> {
                if kept.contains(&part) {
                    return Some(id);
                }
                let k = fragment.part(part).ok()?.bodies.iter().position(|b| b.id == id)?;
                Some(doc.part(components[&part]).ok()?.bodies.get(k)?.id)
            };
            let mut links = Vec::with_capacity(a.links.len());
            for l in &a.links {
                let copied = (|| {
                    let (source, from) = walk(self, &l.source)?;
                    let (receiver, to) = walk(self, &l.receiver)?;
                    self.part(from).ok()?;
                    Some((source, receiver, body(self, to, l.body)?))
                })();
                if let Some((source, receiver, linked)) = copied {
                    links.push(crate::Link { id: self.fresh_id(), source, receiver, body: linked, seat: l.seat });
                }
            }
            self.assembly_mut(components[old])?.links = links;
        }
        Ok((occurrences(self, &fragment.root_assembly().occurrences)?, renamed))
    }
}

/// The deepest nesting walked; deeper is treated as a cycle.
pub const MAX_DEPTH: usize = 64;

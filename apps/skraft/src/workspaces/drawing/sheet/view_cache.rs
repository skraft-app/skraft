//! What a view shows, kept between frames: its lines, its snaps, its box.

use super::*;

/// What each view shows (its parts, its box, what its lines are computed from), kept while
/// the document and its meshes stay as they are: a sheet drawn again computes none of it.
#[derive(Clone, Debug, Default)]
pub(super) struct ViewsCache {
    pub(super) key: (u64, u64),
    /// By view and the direction it looks along: a composition's projected view looks its
    /// presentation's way (docs/ui/drawing.md#compositions).
    pub(super) views: HashMap<(Id, [u64; 19]), Arc<ViewData>>,
}

/// What a view shows.
pub(super) struct ViewData {
    /// Its subject's box in its frame (x, y, depth); none while no part's box is known.
    pub(super) bx3: Option<(DVec3, DVec3)>,
    /// Per part (its definition): its placements in the subject's axes and their materials.
    pub(super) parts: Vec<(Id, Vec<Placed>)>,
    pub(super) lines: Option<Arc<crate::workspaces::drawing::sheet::lines::LinesJob>>,
    /// The parts it shows, placed in its subject's axes.
    pub(super) placed: Vec<skraft_model::PlacedPart>,
    /// Stands for how its parts look: their meshes, faces' paint and materials.
    pub(super) looks: u64,
    /// The texture images its parts show.
    pub(super) images: Arc<[String]>,
    /// Its vertices and edges on the sheet, made when a dimension's tool asks.
    pub(super) snaps: std::sync::OnceLock<Snaps>,
}

/// An edge a dimension's tool may pick: its placed part, its index, its shape in the part's
/// axes, as a polyline in the part's axes and on the sheet.
pub(super) type SnapEdge = (usize, u32, skraft_model::kernel::topo::EdgeShape, Vec<DVec3>, Vec<DVec2>);

/// What a dimension's tool picks in a view: its parts' vertices and edges on the sheet.
#[derive(Default)]
pub(super) struct Snaps {
    /// (placed part, vertex, its point in the part's axes, on the sheet).
    pub(super) vertices: Vec<(usize, u32, DVec3, DVec2)>,
    /// (placed part, edge, its shape in the part's axes, as a polyline in the part's axes and
    /// on the sheet).
    pub(super) edges: Vec<SnapEdge>,
}

impl ViewData {
    /// The centre of its box in its frame (what its frame's centre stands for).
    pub(super) fn centre(&self) -> Option<DVec2> {
        self.bx3.map(|(lo, hi)| ((lo + hi) / 2.0).truncate())
    }

    /// Its vertices and edges on the sheet.
    pub(super) fn snaps(&self, e: &Editor, r: &Resolved<'_>) -> &Snaps {
        self.snaps.get_or_init(|| {
            let Some(centre) = self.centre() else { return Snaps::default() };
            let on_sheet = |p: DVec3| r.at + (r.frame.project(p).truncate() - centre) * r.scale;
            let mut s = Snaps::default();
            for (k, p) in self.placed.iter().enumerate() {
                let Ok(part) = e.document().part(p.part) else { continue };
                let shape = part.shape();
                for (v, q) in shape.vertex_points().into_iter().enumerate() {
                    s.vertices.push((k, v as u32, q, on_sheet(p.transform.transform_point3(q))));
                }
                for i in 0..shape.edge_count() as u32 {
                    let Some(es) = shape.edge_shape(i) else { continue };
                    let pts: Vec<DVec3> = match es {
                        skraft_model::kernel::topo::EdgeShape::Circle { centre, normal, radius, a, b } => {
                            // Its arc sampled, from a to b about its normal (a whole turn when closed).
                            let x = (a - centre).normalize_or_zero();
                            let y = normal.cross(x);
                            let end = (b - centre).normalize_or_zero();
                            let mut sweep = end.dot(y).atan2(end.dot(x));
                            if sweep <= 1e-9 {
                                sweep += std::f64::consts::TAU;
                            }
                            (0..=24)
                                .map(|j| centre + (x * (sweep * j as f64 / 24.0).cos() + y * (sweep * j as f64 / 24.0).sin()) * radius)
                                .collect()
                        }
                        other => {
                            let (a, b) = other.ends();
                            vec![a, b]
                        }
                    };
                    let sheet: Vec<DVec2> = pts.iter().map(|q| on_sheet(p.transform.transform_point3(*q))).collect();
                    s.edges.push((k, i, es, pts, sheet));
                }
            }
            s
        })
    }
}

impl std::fmt::Debug for ViewData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewData").field("bx3", &self.bx3).field("parts", &self.parts.len()).finish()
    }
}

impl ViewsCache {
    /// What view `id` (`r`) shows.
    pub(super) fn get(&mut self, e: &Editor, id: Id, r: &Resolved<'_>) -> Arc<ViewData> {
        let key = (e.history().revision(), e.scene().generation());
        if key != self.key {
            self.key = key;
            self.views.clear();
        }
        self.views
            .entry((id, e_key(r)))
            .or_insert_with(|| {
                let placed = e.document().subject_parts(r.subject);
                let mut by_part: HashMap<Id, Vec<Placed>> = HashMap::new();
                for p in &placed {
                    by_part.entry(p.part).or_default().push((p.transform, p.material));
                }
                let mut parts: Vec<_> = by_part.into_iter().collect();
                parts.sort_by_key(|(p, _)| *p);
                Arc::new(ViewData {
                    bx3: box_of(e, r, &placed),
                    looks: looks_of(e, &parts),
                    images: images_of(e, &parts),
                    parts,
                    lines: crate::workspaces::drawing::sheet::lines::LinesJob::of(e, r, &placed).map(Arc::new),
                    placed,
                    snaps: std::sync::OnceLock::new(),
                })
            })
            .clone()
    }
}

/// A hash of how `parts` look: each one's mesh revision (its shape, its faces' paint) and the
/// materials they use, so that editing another material redraws nothing.
fn looks_of(e: &Editor, parts: &[(Id, Vec<Placed>)]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (part, _) in parts {
        e.scene().definition_mesh(*part).map(|dm| dm.revision).hash(&mut h);
    }
    let used = materials_of(e, parts);
    for (id, m) in e.document().materials().filter(|(id, _)| used.contains(id)) {
        (id.0, m.color).hash(&mut h);
        if let Some(t) = &m.texture {
            (&t.image, t.size.map(f64::to_bits), t.rotation.to_bits()).hash(&mut h);
        }
    }
    h.finish()
}

/// The materials `parts` show: those of their placements and of their painted faces.
fn materials_of(e: &Editor, parts: &[(Id, Vec<Placed>)]) -> std::collections::BTreeSet<Id> {
    let mut used = std::collections::BTreeSet::new();
    for (part, places) in parts {
        used.extend(places.iter().filter_map(|(_, m)| *m));
        if let Some(dm) = e.scene().definition_mesh(*part) {
            used.extend(dm.faces.values().copied());
        }
    }
    used
}

/// The texture images `parts` show, each once: a picture is drawn again as they get ready.
pub fn images_of(e: &Editor, parts: &[(Id, Vec<Placed>)]) -> Arc<[String]> {
    let used = materials_of(e, parts);
    let mut images: Vec<String> = e
        .document()
        .materials()
        .filter(|(id, _)| used.contains(id))
        .filter_map(|(_, m)| m.texture.as_ref().map(|t| t.image.clone()))
        .collect();
    images.sort();
    images.dedup();
    images.into()
}

/// The box of a view's subject seen in its frame, in model units: sheet x, y before scaling, and
/// depth (along its forward); its parts' exact boxes' corners, projected. None while nothing is
/// known of them.
pub fn subject_box3(e: &Editor, r: &Resolved<'_>) -> Option<(DVec3, DVec3)> {
    box_of(e, r, &e.document().subject_parts(r.subject))
}

/// The box of `placed` seen in `r`'s frame.
pub(super) fn box_of(e: &Editor, r: &Resolved<'_>, placed: &[skraft_model::PlacedPart]) -> Option<(DVec3, DVec3)> {
    let mut b: Option<(DVec3, DVec3)> = None;
    for p in placed {
        let Some((lo, hi)) = e.scene().bounds_of(&p.shape) else { continue };
        for i in 0..8 {
            let c = DVec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            );
            let q = r.frame.project(p.transform.transform_point3(c));
            b = Some(b.map_or((q, q), |(a, z)| (a.min(q), z.max(q))));
        }
    }
    b
}

/// [`subject_box3`] on the sheet's plane.
pub fn subject_box(e: &Editor, r: &Resolved<'_>) -> Option<(DVec2, DVec2)> {
    subject_box3(e, r).map(|(lo, hi)| (lo.truncate(), hi.truncate()))
}

/// A view's frame on the sheet, in millimetres: its subject's box (`bx`, seen in its frame) at
/// its scale, centred on where it is placed; a small square while the box is not known.
pub fn frame_of(r: &Resolved<'_>, bx: Option<(DVec2, DVec2)>) -> (DVec2, DVec2) {
    if let Some(c) = r.crop {
        return (r.at + c.lo, r.at + c.hi);
    }
    // A perspective view's is its own (decision 0051).
    if let Some(p) = r.perspective {
        return (r.at - p.size / 2.0, r.at + p.size / 2.0);
    }
    let half = bx.map_or(DVec2::splat(10.0), |(lo, hi)| (hi - lo) * r.scale / 2.0);
    (r.at - half, r.at + half)
}

/// [`frame_of`] its subject's box.
pub fn view_frame(e: &Editor, r: &Resolved<'_>) -> (DVec2, DVec2) {
    frame_of(r, subject_box(e, r))
}

/// A part placed in a view's subject: its transform there, its material.
pub type Placed = (DAffine3, Option<Id>);

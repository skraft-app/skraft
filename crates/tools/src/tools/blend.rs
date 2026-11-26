//! « Arrondir » and « Chanfreiner » (docs/tools/rounds.md): edges of one body of the open part,
//! or of a closed part's shape, picked one by one (a double click: their tangent chain), the
//! size typed; the kernel previews the result, Enter applies it — a body node wrapping that
//! body, or the part's bodies (docs/decisions/0042-body-nodes-operators.md).

use glam::{DAffine3, DVec3};
use skraft_i18n::t;
use skraft_model::kernel::names::{Blend, BlendOp, edges_named};
use skraft_model::{EntityPath, Id, Operation, RawCache};

use super::{Command, Cx, Key, OptionValue, Preview, Steps, Tool, ToolInput, ToolKind, ToolOption};
use crate::draw::{Color, Pen, ThemeColor};
use crate::infer::{EdgeHit, EdgesOn, edge_at, tangent_chain};
use crate::scene::{Owner, Piece};
use crate::vcb::{Vcb, VcbKind, VcbValue};

/// What the preview of a round shows (the editor has the kernel compute it,
/// docs/tools/rounds.md): the operation, and what its result is drawn instead of.
#[derive(Clone, Debug, PartialEq)]
pub struct BlendPreview {
    pub op: Operation,
    /// The closed part's occurrence drawn otherwise, at `world`; or, in the open part, the body.
    pub path: EntityPath,
    pub body: Option<Id>,
    pub world: DAffine3,
}

/// The round and chamfer tools.
pub struct BlendTool {
    blend: Blend,
    /// The radius or the distance (mm).
    size: f64,
    hover: Option<EdgeHit>,
    /// Why the edge under the cursor is not taken: a linked body's, its source named
    /// (docs/model/bodies.md#links).
    refused: Option<String>,
    /// The edges picked, all of one body or one part (in a node opened, of its children's
    /// result).
    picks: Vec<EdgeHit>,
    /// The node opened whose edges the picks were loaded from (docs/tools/rounds.md), whether
    /// they (or its size) were changed since, and whether its children's result was there to
    /// load them from (it may come after the node is opened).
    loaded: Option<Id>,
    changed: bool,
    raw_loaded: bool,
    /// The node's size was typed here: `size` is what it will take. Else `size` follows the
    /// node's, which the entity info may change while the tool is on.
    size_typed: bool,
}

/// The size option (saved per tool): the radius, the distance.
pub const SIZE_OPTION: &str = "size";

/// The size a round or a chamfer starts with (mm).
const SIZE: f64 = 2.0;

impl BlendTool {
    pub fn new(blend: Blend) -> Self {
        BlendTool {
            blend,
            size: SIZE,
            hover: None,
            refused: None,
            picks: Vec::new(),
            loaded: None,
            changed: false,
            raw_loaded: false,
            size_typed: false,
        }
    }

    /// The body node open (the group open is one), a round's or a chamfer's: the path to what
    /// holds it, its id, its operation. The tool then picks its edges again rather than
    /// wrapping a body.
    pub(crate) fn node(cx: &Cx<'_>) -> Option<(EntityPath, Id, BlendOp)> {
        let (&node, above) = cx.open_group.split_last()?;
        let parent = cx.open_part_path()?.in_group(above.to_vec());
        let (_, p, _) = cx.doc.resolve_container(&parent).ok()?;
        let op = p.body(node).and_then(|b| p.def_of(b).op())?.blend()?.clone();
        Some((parent, node, op))
    }

    /// The open node's children's result and its edges, in the node's axes, once they are there.
    fn raw_edges<'a>(cx: &'a Cx<'_>, path: &'a EntityPath) -> Option<(&'a RawCache, EdgesOn<'a>)> {
        let (raw, snaps) = cx.scene.node_raw()?;
        Some((raw, EdgesOn { snaps, path, transform: cx.axes() }))
    }

    /// The node's edges as they are, on its children's result: an edge between two children
    /// lies on neither of them as shown.
    pub(crate) fn load(cx: &Cx<'_>, op: &BlendOp) -> Vec<EdgeHit> {
        let Some(path) = cx.open_part_path() else { return Vec::new() };
        let Some((raw, on)) = Self::raw_edges(cx, &path) else { return Vec::new() };
        let to_world = cx.axes();
        let mut out: Vec<EdgeHit> = Vec::new();
        for e in &op.edges {
            let hits = on.with_ids(&edges_named(raw.solid.record(), &raw.faces, &e.name));
            if e.near.is_empty() {
                out.extend(hits);
            } else {
                for q in &e.near {
                    let q = to_world.transform_point3(DVec3::from_array(*q));
                    if let Some(h) = hits.iter().min_by(|x, y| x.middle.distance(q).total_cmp(&y.middle.distance(q))) {
                        out.push(h.clone());
                    }
                }
            }
        }
        out
    }

    /// The picks follow the node opened: its edges loaded when it is, let go when it closes.
    fn sync(&mut self, cx: &Cx<'_>) {
        let node = Self::node(cx);
        match &node {
            // Its edges, as they are.
            Some((_, id, op)) if self.loaded != Some(*id) => {
                self.picks = Self::load(cx, op);
                self.raw_loaded = cx.scene.node_raw().is_some();
                self.loaded = Some(*id);
                self.changed = false;
                self.size_typed = false;
            }
            // Its children's result come since it was opened: its edges known only now.
            Some((_, _, op)) if self.awaits_raw(cx) => {
                self.picks = Self::load(cx, op);
                self.raw_loaded = true;
            }
            None if self.loaded.is_some() => {
                self.picks.clear();
                self.loaded = None;
                self.changed = false;
                self.size_typed = false;
            }
            _ => {}
        }
        if let Some((_, _, op)) = &node {
            self.size = self.node_size(op);
        }
    }

    /// The size the node `op` is edited to: the one typed here, else its own.
    fn node_size(&self, op: &BlendOp) -> f64 {
        if self.size_typed { self.size } else { op.size }
    }

    /// Has the node's children's result come since its edges were loaded?
    fn awaits_raw(&self, cx: &Cx<'_>) -> bool {
        !self.raw_loaded && cx.scene.node_raw().is_some()
    }

    /// What can be picked: a body of the open part, else a closed part's shape.
    fn keeps(cx: &Cx<'_>, o: &Owner<'_>) -> bool {
        match cx.open_part_path() {
            Some(open) => {
                matches!(o.piece, Piece::Blank(_) | Piece::Tool(_))
                    && o.path.occurrences == open.occurrences
                    && o.path.entity == open.entity
            }
            None => o.piece == Piece::Shape,
        }
    }

    /// The edge under the cursor: in a node opened, of its children's result; else, once one
    /// is picked, only of its body or part — the others still hiding what lies behind them.
    fn edge(&self, cx: &Cx<'_>, px: glam::DVec2) -> Option<EdgeHit> {
        if Self::node(cx).is_some() {
            let path = cx.open_part_path()?;
            let (_, on) = Self::raw_edges(cx, &path)?;
            return on.at(cx.scene, cx.camera, cx.size, px, &|o| Self::keeps(cx, o));
        }
        let first = self.picks.first().filter(|_| self.loaded.is_none());
        let keep = |o: &Owner<'_>| {
            Self::keeps(cx, o) && Self::linked(cx, o).is_none() && first.is_none_or(|f| *o.path == f.path && o.piece == f.piece)
        };
        edge_at(cx.scene, cx.camera, cx.size, px, &keep, &|o| Self::keeps(cx, o))
    }

    /// The source's name, if `o` is a linked body of the open part: its edges follow their
    /// source, never rounded (docs/model/bodies.md#links).
    fn linked(cx: &Cx<'_>, o: &Owner<'_>) -> Option<String> {
        match o.piece {
            Piece::Blank(id) | Piece::Tool(id) => cx.linked_body(id),
            _ => None,
        }
    }

    /// Why no edge is taken at `px`: the linked body whose edge is there, its source named.
    fn refusal(cx: &Cx<'_>, px: glam::DVec2) -> Option<String> {
        if Self::node(cx).is_some() {
            return None;
        }
        let mut name = None;
        let keep = |o: &Owner<'_>| Self::keeps(cx, o) && Self::linked(cx, o).is_some();
        let hit = edge_at(cx.scene, cx.camera, cx.size, px, &keep, &|o| Self::keeps(cx, o))?;
        if let Piece::Blank(id) | Piece::Tool(id) = hit.piece {
            name = cx.linked_body(id);
        }
        Some(t!("tool.blend.linked_body", source = name?))
    }

    /// The edges running on from `e` tangentially, among those it was picked from.
    fn chain(cx: &Cx<'_>, e: &EdgeHit) -> Vec<EdgeHit> {
        if Self::node(cx).is_some()
            && let Some((_, on)) = Self::raw_edges(cx, &e.path)
        {
            return on.chain(e);
        }
        tangent_chain(cx.scene, e)
    }

    /// The size saved, or the first one's.
    fn size(cx: &Cx<'_>) -> f64 {
        Some(cx.length(SIZE_OPTION)).filter(|s| *s > 0.0).unwrap_or(SIZE)
    }

    fn same_owner(a: &EdgeHit, b: &EdgeHit) -> bool {
        a.path == b.path && a.piece == b.piece
    }

    /// `edges` picked or, all picked already, let go; another body or part's starts again.
    fn toggle(&mut self, edges: Vec<EdgeHit>) {
        let Some(first) = edges.first() else { return };
        self.changed = true;
        // Only the first's body or part is picked from ([`BlendTool::edge`]); a node's edges may
        // be of several of its children.
        if self.loaded.is_none() && self.picks.first().is_some_and(|p| !Self::same_owner(p, first)) {
            return;
        }
        let picked = |e: &EdgeHit, picks: &[EdgeHit]| picks.iter().any(|p| p.edge == e.edge && Self::same_owner(p, e));
        if edges.iter().all(|e| picked(e, &self.picks)) {
            self.picks.retain(|p| !edges.iter().any(|e| e.edge == p.edge));
        } else {
            for e in edges {
                if !picked(&e, &self.picks) {
                    self.picks.push(e);
                }
            }
        }
    }

    /// The operation the picks make, with what its preview is drawn instead of.
    fn operation(&self, cx: &Cx<'_>) -> Option<BlendPreview> {
        let first = self.picks.first()?;
        // In a node opened: its edges picked again, in its axes (the group open's).
        if let Some((parent, node, now)) = Self::node(cx) {
            let to_node = cx.axes().inverse();
            let picks: Vec<DVec3> = self.picks.iter().map(|p| to_node.transform_point3(p.middle)).collect();
            // Its result previewed in the stead of its first child.
            let body = cx.scene.recipe().and_then(|r| r.bodies().next()).map(|b| b.body);
            let size = self.node_size(&now);
            let op = Operation::SetBlend { target: parent, body: node, blend: self.blend, size, picks, scope: cx.part_scope };
            return Some(BlendPreview { op, path: cx.open_part_path()?, body, world: DAffine3::IDENTITY });
        }
        let (target, bodies, body, to_target, world) = match (first.piece, cx.open_part_path()) {
            (Piece::Blank(id) | Piece::Tool(id), Some(open)) => (open, vec![id], Some(id), cx.axes().inverse(), DAffine3::IDENTITY),
            (Piece::Shape, None) => {
                let (_, part, _) = cx.doc.resolve_part(&first.path).ok()?;
                let bodies: Vec<Id> = part.bodies.iter().map(|b| b.id).collect();
                (first.path.clone(), bodies, None, first.transform.inverse(), first.transform)
            }
            _ => return None,
        };
        let picks: Vec<DVec3> = self.picks.iter().map(|p| to_target.transform_point3(p.middle)).collect();
        let op = Operation::Blend { target: target.clone(), bodies, blend: self.blend, size: Self::size(cx), picks, scope: cx.part_scope };
        Some(BlendPreview { op, path: target, body, world })
    }

    /// What validating the node open applies. No edge picked, its edges known: the user let go
    /// of its last one, the node is undone (every copy); its edges not known (its children's
    /// result still to come, names no longer matching), only a size typed is applied.
    fn node_validated(&self, cx: &Cx<'_>) -> Result<Option<Operation>, String> {
        if !self.picks.is_empty() {
            return Ok(self.operation(cx).map(|p| p.op));
        }
        let Some((target, body, now)) = Self::node(cx) else { return Ok(None) };
        if !Self::load(cx, &now).is_empty() {
            cx.doc.check_dissolve_node(&target, body).map_err(|e| e.to_string())?;
            return Ok(Some(Operation::DissolveNode { target, body, scope: cx.part_scope }));
        }
        Ok(self.size_typed.then(|| Operation::SetBlend {
            target,
            body,
            blend: self.blend,
            size: self.size,
            picks: Vec::new(),
            scope: cx.part_scope,
        }))
    }
}

impl Tool for BlendTool {
    fn kind(&self) -> ToolKind {
        match self.blend {
            Blend::Round => ToolKind::Round,
            Blend::Chamfer => ToolKind::Chamfer,
        }
    }

    /// On a node open: its edges and its size at once, for the bar and the measurements box.
    fn started(&mut self, cx: &Cx<'_>) {
        self.sync(cx);
    }

    fn input(&mut self, input: &ToolInput, cx: &Cx<'_>) -> Vec<Command> {
        self.sync(cx);
        match input {
            ToolInput::Hover { px } => {
                // A node's own size while it is edited; else the tool's.
                if self.loaded.is_none() {
                    self.size = Self::size(cx);
                }
                self.hover = self.edge(cx, *px);
                self.refused = if self.hover.is_none() { Self::refusal(cx, *px) } else { None };
            }
            ToolInput::Click { px, .. } => {
                if let Some(e) = self.edge(cx, *px) {
                    self.toggle(vec![e]);
                }
            }
            ToolInput::DoubleClick { px, .. } => {
                // The click before took the edge: the double click takes its whole chain.
                if let Some(e) = self.edge(cx, *px) {
                    let chain = Self::chain(cx, &e);
                    if !self.picks.iter().any(|p| p.edge == e.edge && Self::same_owner(p, &e)) {
                        self.toggle(vec![e]);
                    }
                    let rest: Vec<EdgeHit> =
                        chain.into_iter().filter(|c| !self.picks.iter().any(|p| p.edge == c.edge && Self::same_owner(p, c))).collect();
                    if !rest.is_empty() {
                        self.toggle(rest);
                    }
                }
            }
            // Kept as the tool's option: the next round takes it again.
            ToolInput::Value(VcbValue::Lengths(v)) => {
                if let Some(s) = v.first().map(|s| s.abs()).filter(|s| *s > 0.0) {
                    self.size = s;
                    // A node's size changed: its own, not the tool's.
                    if self.loaded.is_some() {
                        self.changed = true;
                        self.size_typed = true;
                        return Vec::new();
                    }
                    return vec![Command::SetOption { key: SIZE_OPTION, value: OptionValue::Length(s) }];
                }
            }
            ToolInput::Key { key: Key::Enter, .. } => {
                // A node edited: applied, then closed — back in its parent, it selected, the
                // selection tool on (where « Choisir les arêtes » was asked).
                if self.loaded.is_some() {
                    if !self.changed {
                        return Vec::new();
                    }
                    let op = match self.node_validated(cx) {
                        Ok(Some(op)) => op,
                        Ok(None) => return Vec::new(),
                        Err(refused) => return vec![Command::Say(refused)],
                    };
                    self.picks.clear();
                    self.loaded = None;
                    self.changed = false;
                    self.size_typed = false;
                    // Undone, it is closed first: what it held is selected, not the node gone.
                    if matches!(op, Operation::DissolveNode { .. }) {
                        return vec![Command::Exit, Command::Start(ToolKind::Select), Command::Apply(op)];
                    }
                    return vec![Command::Apply(op), Command::Exit, Command::Start(ToolKind::Select)];
                }
                if let Some(p) = self.operation(cx) {
                    self.picks.clear();
                    return vec![Command::Apply(p.op)];
                }
            }
            _ => {}
        }
        Vec::new()
    }

    fn preview(&self, cx: &Cx<'_>) -> Preview {
        let mut preview = Preview::default();
        {
            let mut d = cx.drawer(&mut preview.drawing);
            // A node just opened, before any input: its edges lit already.
            let loading = Self::node(cx)
                .filter(|(_, node, _)| self.loaded != Some(*node) || self.awaits_raw(cx))
                .map(|(_, _, op)| Self::load(cx, &op));
            for p in loading.as_ref().unwrap_or(&self.picks) {
                d.pen(Pen::new(Color::Theme(ThemeColor::Selected)).bold().over()).polyline(&p.drawn);
            }
            if let Some(h) = &self.hover {
                d.pen(Pen::new(Color::Theme(ThemeColor::Hover)).bold().over()).polyline(&h.drawn);
            }
        }
        preview.blend = self.operation(cx);
        preview.tip = self.refused.clone();
        preview
    }

    fn cursor(&self, _: &Cx<'_>) -> Option<crate::tools::Cursor> {
        self.refused.as_ref().map(|_| crate::tools::Cursor::Refused)
    }

    fn status(&self) -> String {
        match (self.picks.is_empty(), self.blend) {
            (true, Blend::Round) => t!("tool.round.hint.pick"),
            (true, Blend::Chamfer) => t!("tool.chamfer.hint.pick"),
            (false, _) => t!("tool.blend.hint.validate"),
        }
    }

    fn vcb(&self) -> Option<Vcb> {
        let label = match self.blend {
            Blend::Round => t!("tool.round.vcb"),
            Blend::Chamfer => t!("tool.chamfer.vcb"),
        };
        Some(Vcb::new(label, VcbKind::Lengths(vec![self.size])))
    }

    fn busy(&self) -> bool {
        if self.loaded.is_some() { self.changed } else { !self.picks.is_empty() }
    }

    /// The size, in the bar: the tool's (saved: the next round takes it again), or on a node
    /// open the node's own (not saved).
    fn options(&self, cx: &Cx<'_>) -> Vec<ToolOption> {
        let (label, tip) = match self.blend {
            Blend::Round => (t!("tool.round.vcb"), t!("tool.round.option.size_tip")),
            Blend::Chamfer => (t!("tool.chamfer.vcb"), t!("tool.chamfer.option.size_tip")),
        };
        let node = Self::node(cx);
        let size = node.as_ref().map_or_else(|| Self::size(cx), |(_, _, op)| self.node_size(op));
        vec![ToolOption {
            key: SIZE_OPTION,
            label,
            tip,
            value: OptionValue::Length(size),
            choices: Vec::new(),
            saved: node.is_none(),
            icon: None,
            swatch: None,
        }]
    }

    fn set_option(&mut self, key: &str, value: &OptionValue) {
        if key == SIZE_OPTION
            && let OptionValue::Length(v) = value
            && *v > 0.0
        {
            self.size = *v;
            // On a node: its own size changed, to validate.
            if self.loaded.is_some() {
                self.changed = true;
                self.size_typed = true;
            }
        }
    }

    fn picking(&self) -> bool {
        true
    }

    /// The edges, then the validation (OCL's steps bar of the solid actions).
    fn steps(&self) -> Option<Steps> {
        Some(Steps { count: 1, index: 0, text: t!("tool.blend.step.edges"), names: Vec::new() })
    }

    /// A node's edges wait for it once changed; new ones as soon as one is picked.
    fn awaits_validation(&self) -> bool {
        if self.loaded.is_some() { self.changed } else { !self.picks.is_empty() }
    }

    fn leave(&mut self) {
        self.hover = None;
        self.refused = None;
    }

    /// Every edge let go; a node's, its own taken again (its changes undone): it stays the one
    /// loaded, its edges read again from its children's result.
    fn back(&mut self) {
        self.picks.clear();
        self.raw_loaded = false;
        self.changed = false;
        self.size_typed = false;
    }

    fn reset(&mut self) {
        self.picks.clear();
        self.hover = None;
        self.loaded = None;
        self.changed = false;
        self.size_typed = false;
    }
}

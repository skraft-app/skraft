//! OpenCutList's tree drop-up (`SmartSelectActionHandler`, `STATE_SELECT_TREE`;
//! docs/tools/selection.md): under « Parts », the copy modifier (Option on macOS,
//! Ctrl elsewhere) held over a part while a tool picks lists its path from the root — the
//! levels above the active context disabled —; a row hovered is drawn hovered in the view, a
//! row clicked is selected (the part, or one of the assemblies holding it); the key released
//! closes the list. The tap its release makes is not the tool's (no copy toggled).

use glam::DVec2;
use skraft_model::EntityPath;

use super::Editor;

/// One level of the path listed.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeRow {
    /// The occurrence, from the root.
    pub path: EntityPath,
    /// Its own name (« Gauche »), empty if it has none.
    pub name: String,
    /// Its component's name.
    pub component: String,
    /// That name is automatic (docs/model/bodies.md#automatic-names).
    pub auto: bool,
    /// How deep it is (0: in the model).
    pub depth: usize,
    /// Below the active context: it can be chosen.
    pub enabled: bool,
}

/// The list open.
#[derive(Clone, Debug, PartialEq)]
pub struct Tree {
    /// The model's level first, the part last.
    pub rows: Vec<TreeRow>,
    /// Where it was opened (pixels in the view): the list stands above it.
    pub at: DVec2,
    /// The row under the pointer.
    pub hovered: Option<usize>,
}

impl Editor {
    /// The tree drop-up, while it is open.
    pub fn tree(&self) -> Option<&Tree> {
        self.tree.as_ref()
    }

    /// Can the copy modifier open the list now: « Parts », the tool picking (Select, or a
    /// tool with nothing taken yet), nothing under way?
    pub(super) fn tree_allowed(&self) -> bool {
        self.picks_parts_now() && !self.tool.busy() && !(self.tool.picks_first() && self.anything_selected())
    }

    /// The copy modifier pressed: the list of the path of the part under the cursor, when it
    /// is in an assembly under the active context (a single row would choose nothing).
    pub(super) fn open_tree(&mut self) {
        self.tree_tap = false;
        if !self.tree_allowed() {
            return;
        }
        let Some(at) = self.last_hover else { return };
        let Some(part) = self.cx().part_at(at) else { return };
        if part.occurrences.len() <= self.context.len() {
            return;
        }
        let doc = self.document();
        let chain: Vec<_> = part.occurrences.iter().copied().chain([part.entity]).collect();
        let mut rows = Vec::new();
        for k in 0..chain.len() {
            let path = EntityPath::new(chain[..k].to_vec(), chain[k]);
            let Ok(o) = doc.resolve(&path) else { return };
            let (component, auto) = doc.component(o.component).map(|c| (c.name.clone(), !c.named)).unwrap_or_default();
            rows.push(TreeRow { path, name: o.name.clone(), component, auto, depth: k, enabled: k >= self.context.len() });
        }
        self.tree = Some(Tree { rows, at, hovered: None });
        // Its release taps the key: not the tool's.
        self.tree_tap = true;
    }

    /// The copy modifier released: the list closed.
    pub(super) fn close_tree(&mut self) {
        self.tree = None;
    }

    /// The row `row` hovered in the list (none: the pointer left it).
    pub fn hover_tree_row(&mut self, row: Option<usize>) {
        if let Some(t) = &mut self.tree {
            t.hovered = row.filter(|k| t.rows.get(*k).is_some_and(|r| r.enabled));
        }
    }

    /// The row `row` clicked: selected (the part, or an assembly holding it), the list closed;
    /// a tool picking takes it. A tool taking it its own way does (Paint paints it).
    pub fn pick_tree_row(&mut self, row: usize) {
        let Some(path) = self.tree.as_ref().and_then(|t| t.rows.get(row)).filter(|r| r.enabled).map(|r| r.path.clone()) else {
            return;
        };
        self.tree = None;
        match self.tool.tree_pick(&path, &self.cx()) {
            Some(commands) => {
                for c in commands {
                    self.run(c);
                }
            }
            None => self.select_paths(vec![path]),
        }
        self.rehover();
    }

    /// The path of the row hovered in the list (drawn hovered in the view).
    pub(super) fn tree_hovered(&self) -> Option<&EntityPath> {
        let t = self.tree.as_ref()?;
        t.hovered.and_then(|k| t.rows.get(k)).map(|r| &r.path)
    }
}

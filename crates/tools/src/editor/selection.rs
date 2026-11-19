//! Selection and the context being edited: select, enter and leave assemblies, open and close
//! a part (docs/model/opening-a-part.md), erase, make an assembly, explode, make a part on its own.

use skraft_i18n::t;
use skraft_model::{EntityPath, GuidePath, Id, Operation};

use super::{After, Editor};
use crate::ToolKind;
use crate::tools::Command;

impl Editor {
    /// Selects `ids`, entities of the active context: the parts selected at any depth let go.
    pub(super) fn set_selection(&mut self, ids: Vec<Id>) {
        self.deep.clear();
        self.deep_guides.clear();
        self.selection = ids;
    }

    /// Nothing selected, at any depth.
    pub(super) fn clear_selection(&mut self) {
        self.deep.clear();
        self.deep_guides.clear();
        self.selection.clear();
    }

    /// Selects the parts `paths` name, at any depth under the active context (« Parts »,
    /// docs/tools/selection.md): the ids' selection let go.
    pub fn select_paths(&mut self, paths: Vec<EntityPath>) {
        self.select_parts_and_guides(paths, Vec::new());
    }

    /// Selects the parts `paths` and the guides `guides` name, at any depth under the active
    /// context (« Parts », docs/tools/selection.md): the ids' selection let go.
    pub(super) fn select_parts_and_guides(&mut self, paths: Vec<EntityPath>, guides: Vec<GuidePath>) {
        self.selection.clear();
        self.deep = paths;
        self.deep_guides = guides;
    }

    /// The guides selected at any depth (« Parts »), by their paths from the root.
    pub fn deep_guides(&self) -> &[GuidePath] {
        &self.deep_guides
    }

    /// The parts selected at any depth (« Parts »), by their paths from the root; empty
    /// while the selection is the active context's.
    pub fn deep_selection(&self) -> &[EntityPath] {
        &self.deep
    }

    /// Is anything selected, here or at any depth?
    pub fn anything_selected(&self) -> bool {
        !self.selection.is_empty() || !self.deep.is_empty() || !self.deep_guides.is_empty()
    }

    /// Does the selection at any depth go through an assembly placed more than once (a change
    /// to it reaches the other copies, unless made unique: the scope, docs/tools/selection.md)?
    pub fn deep_shared(&self) -> bool {
        let doc = self.document();
        self.deep.iter().any(|p| doc.shared_along(&p.instances))
            || self
                .deep_guides
                .iter()
                .any(|g| doc.component_at(&g.instances).is_ok_and(|(c, _)| doc.assembly(c).is_ok()) && doc.shared_along(&g.instances))
    }

    pub fn select(&mut self, ids: Vec<Id>) {
        self.set_selection(ids);
    }

    pub fn select_all(&mut self) {
        let guides = self.shown_guides();
        if self.open_part.is_some() {
            self.selection =
                self.scene.recipe().filter(|_| self.showing_machinings()).map(|r| r.bodies().map(|t| t.body).collect()).unwrap_or_default();
            self.selection.extend(guides);
            return;
        }
        let ctx = self.context_definition();
        // Not those hidden, unless the hidden are shown.
        let show_hidden = self.document().settings().display.show_hidden;
        self.deep.clear();
        self.deep_guides.clear();
        self.selection = self
            .document()
            .assembly(ctx)
            .map(|a| a.occurrences().iter().filter(|o| show_hidden || !o.hidden).map(|o| o.id).collect())
            .unwrap_or_default();
        self.selection.extend(guides);
    }

    /// The component guides are drawn in now (docs/tools/tape-measure.md): the open part's, else the
    /// context's.
    pub fn guide_component(&self) -> Id {
        let doc = self.document();
        self.open_part_path().and_then(|p| doc.resolve(&p).ok()).map_or_else(|| self.context_definition(), |o| o.component)
    }

    /// The guides of the active context, while guides are shown.
    pub fn shown_guides(&self) -> Vec<Id> {
        if !self.document().settings().display.guides {
            return Vec::new();
        }
        self.document().component(self.guide_component()).map(|c| c.guides.iter().map(|g| g.id).collect()).unwrap_or_default()
    }

    /// Selects the entity `path` names, editing the context it is in (the outliner's click).
    pub fn select_path(&mut self, path: &EntityPath) {
        if self.document().resolve(path).is_err() {
            return;
        }
        if self.context != path.instances || self.open_part.is_some() {
            self.tool.reset();
            self.context = path.instances.clone();
            self.open_part = None;
            self.open_group.clear();
            self.drop_empty_assemblies(false);
            self.sync_scene();
        }
        if self.document().resolve(path).is_ok() {
            self.set_selection(vec![path.entity]);
        }
    }

    /// The model chosen (the outliner's first row clicked): every assembly left and the part
    /// closed (its profile's editing with it), nothing selected — the model is, until anything
    /// else is.
    pub fn select_model(&mut self) {
        if !self.context.is_empty() || self.open_part.is_some() {
            self.tool.reset();
            if self.tool.kind() == ToolKind::Profile {
                self.set_tool(ToolKind::Select);
            }
            self.context.clear();
            self.open_part = None;
            self.open_group.clear();
            self.listed_hover = None;
            self.drop_empty_assemblies(false);
            self.sync_scene();
        }
        self.clear_selection();
        self.model_selected = true;
    }

    /// Whether the model is what is chosen (the outliner's first row highlighted, the entity
    /// info showing the model): its row clicked, nothing chosen since.
    pub fn model_chosen(&self) -> bool {
        self.model_selected && self.at_model_level_with_nothing_selected()
    }

    pub(crate) fn at_model_level_with_nothing_selected(&self) -> bool {
        self.context.is_empty() && self.open_part.is_none() && !self.anything_selected()
    }

    /// The outliner's click on the occurrence `path` leads to: selected already (in the active
    /// context, no part open), it is taken out of the selection; else selected as
    /// [`Editor::select_path`] does — added to the selection with `add` (Shift) when it lies in
    /// the active context.
    pub fn toggle_path(&mut self, path: &EntityPath, add: bool) {
        let here = self.context == path.instances && self.open_part.is_none();
        if here && self.selection.contains(&path.entity) {
            self.selection.retain(|id| *id != path.entity);
        } else if here && add && self.document().resolve(path).is_ok() {
            self.selection.push(path.entity);
        } else {
            self.select_path(path);
        }
    }

    /// The outliner's click on the body `body` of the part — or group of bodies — `path`
    /// leads to (docs/model/bodies.md#body-groups): the part, then the group, opened first when they are
    /// not; the body then taken out of the selection when selected, else selected alone — added
    /// to it with `add` (Shift).
    pub fn toggle_body(&mut self, path: &EntityPath, body: Id, add: bool) {
        if !self.open_container(path, body) {
            return;
        }
        if self.selection.contains(&body) {
            self.selection.retain(|id| *id != body);
        } else if add {
            self.selection.push(body);
        } else {
            self.set_selection(vec![body]);
        }
    }

    /// Edits the profile of the body `body` of the part or group `path` leads into (the
    /// outliner's « Profil » row): opened as for a click on the body's row, the tool « Profil »
    /// started on it.
    pub fn edit_body_profile(&mut self, path: &EntityPath, body: Id) {
        if self.open_container(path, body) {
            self.run(Command::EditProfile(body));
        }
    }

    /// Opens the part and the group `path` leads into, if it holds `body`: is it there?
    fn open_container(&mut self, path: &EntityPath, body: Id) -> bool {
        let Ok((_, p, _)) = self.document().resolve_container(path) else { return false };
        if p.body(body).is_none() {
            return false;
        }
        let part = path.part_path();
        if self.open_part_path().map(|p| p.part_path()).as_ref() != Some(&part) {
            self.select_path(&part);
            self.enter(part.entity);
        }
        if self.open_group != path.group {
            self.open_group = path.group.clone();
            self.tool.reset();
            self.clear_selection();
            self.listed_hover = None;
            self.sync_scene();
        }
        true
    }

    /// Edits the assembly the occurrence `id` of the active context places, or opens the part
    /// it places.
    pub fn enter(&mut self, id: Id) {
        // In an open part: a group of its bodies opened (docs/model/bodies.md#body-groups).
        if self.open_part.is_some() {
            let Some(path) = self.open_part_path() else { return };
            let Ok((_, p, _)) = self.document().resolve_container(&path) else { return };
            if !p.body(id).is_some_and(|b| p.def_of(b).is_group()) {
                return;
            }
            self.open_group.push(id);
            self.tool.reset();
            self.clear_selection();
            self.listed_hover = None;
            self.sync_scene();
            return;
        }
        let doc = self.document();
        let ctx = self.context_definition();
        let is_part = doc.assembly(ctx).ok().and_then(|a| a.occurrence(id)).is_some_and(|o| doc.part(o.component).is_ok());
        if doc.is_assembly_occurrence(ctx, id) {
            self.context.push(id);
        } else if is_part {
            self.open_part = Some(id);
            self.part_scope = skraft_model::Scope::All;
            self.body_sign = skraft_model::Sign::Negative;
        } else {
            return;
        }
        self.tool.reset();
        self.clear_selection();
        self.listed_hover = None;
        self.sync_scene();
    }

    /// Leaves the body open, else closes the group or part open, else goes back to the parent
    /// context; what is left is selected.
    pub fn exit(&mut self) {
        // A body open first (docs/model/bodies.md#edit-levels).
        if self.leave_body() {
            return;
        }
        let left = match self.open_group.pop() {
            // Out of the group: back in its parent, the group selected.
            Some(id) => Some(id),
            None => self.open_part.take().or_else(|| self.context.pop()),
        };
        if let Some(id) = left {
            self.tool.reset();
            // The profile's editing goes with the part (docs/tools/prism.md, step 3).
            if self.tool.kind() == ToolKind::Profile {
                self.set_tool(ToolKind::Select);
            }
            self.set_selection(vec![id]);
            self.listed_hover = None;
            self.drop_empty_assemblies(false);
            self.sync_scene();
        }
    }

    pub fn delete_selection(&mut self) {
        if (!self.deep.is_empty() || !self.deep_guides.is_empty()) && !self.busy() {
            let paths = std::mem::take(&mut self.deep);
            let guides = std::mem::take(&mut self.deep_guides);
            let op = self.cx().erase_deep(&paths, &guides);
            let _ = self.start(op, After::Nothing, false);
            return;
        }
        if self.selection.is_empty() || self.busy() {
            return;
        }
        let entities = std::mem::take(&mut self.selection);
        self.erase(entities);
    }

    /// Erases entities of the active context: occurrences and guides, or, in an open part,
    /// its machinings and guides.
    fn erase(&mut self, entities: Vec<Id>) {
        let op = self.cx().erase(entities);
        let _ = self.start(op, After::Nothing, false);
    }

    /// Hides the occurrence `path` names, or shows it again (the outliner's eye). Hidden, it
    /// leaves the selection, unless the hidden are shown.
    pub fn set_hidden(&mut self, path: &EntityPath, hidden: bool) {
        if self.busy() {
            return;
        }
        let Ok(context) = self.document().context_of(&path.instances) else { return };
        let op = Operation::SetHidden { context, entities: vec![path.entity], hidden };
        if self.start(op, After::Nothing, false).is_ok()
            && hidden
            && !self.document().settings().display.show_hidden
            && self.context == path.instances
        {
            self.selection.retain(|id| *id != path.entity);
        }
    }

    /// Shows every occurrence hidden (View › Show all).
    pub fn show_all(&mut self) {
        if self.busy() {
            return;
        }
        let _ = self.start(Operation::ShowAll, After::Nothing, false);
    }

    /// Is anything hidden, anywhere in the model? Worked out once per revision.
    pub fn has_hidden(&self) -> bool {
        let revision = self.history.revision();
        if let Some((r, any)) = self.has_hidden.get()
            && r == revision
        {
            return any;
        }
        let doc = self.document();
        let assemblies = doc.components().filter_map(|c| c.as_assembly()).chain([doc.root_assembly()]);
        let any = assemblies.flat_map(|a| a.occurrences()).any(|o| o.hidden);
        self.has_hidden.set(Some((revision, any)));
        any
    }

    /// Erases every guide of the active context (Edit › Delete guides), shown or not.
    pub fn delete_guides(&mut self) {
        if self.busy() {
            return;
        }
        let guides: Vec<Id> =
            self.document().component(self.guide_component()).map(|c| c.guides.iter().map(|g| g.id).collect()).unwrap_or_default();
        if guides.is_empty() {
            self.message = Some(t!("status.no_guides"));
            return;
        }
        self.selection.retain(|id| !guides.contains(id));
        self.erase(guides);
    }

    /// Puts the selection in a new assembly; in an open part, groups its bodies selected.
    pub fn make_assembly(&mut self) {
        if self.busy() {
            return;
        }
        if self.open_part.is_some() {
            return self.group_bodies();
        }
        if self.selection.is_empty() && (!self.deep.is_empty() || !self.deep_guides.is_empty()) {
            return self.make_assembly_deep();
        }
        if self.selection.is_empty() {
            self.message = Some(t!("status.nothing_to_group"));
            return;
        }
        let op = Operation::MakeAssembly { context: self.context_definition(), entities: self.selection.clone(), name: None };
        let _ = self.start(op, After::SelectNamed, false);
    }

    /// « Créer un assemblage » under « Pièces » (docs/tools/selection.md): the parts (and guides)
    /// selected at any depth, all in one assembly, put in a new one there; else refused, told.
    fn make_assembly_deep(&mut self) {
        let crate::tools::Moved::Deep { mut groups, unique } = self.cx().moved_deep(&self.deep, &self.deep_guides) else { return };
        let one = groups.len() == 1 && groups[0].part.is_none();
        let Some(g) = groups.pop().filter(|_| one) else {
            self.message = Some(t!("status.group_one_assembly"));
            return;
        };
        let make = Operation::MakeAssembly { context: Id(0), entities: g.entities, name: None };
        let _ = self.start(Operation::At { parent: g.parent, unique, op: Box::new(make) }, After::SelectNamed, false);
    }

    /// The selected occurrences of assemblies, in the active context.
    pub(super) fn selected_assemblies(&self) -> Vec<Id> {
        let ctx = self.context_definition();
        self.selection.iter().copied().filter(|id| self.document().is_assembly_occurrence(ctx, *id)).collect()
    }

    /// Explodes the assemblies selected; in an open part, flattens its groups selected.
    pub fn explode(&mut self) {
        if self.busy() {
            return;
        }
        if self.open_part.is_some() {
            return self.ungroup_bodies();
        }
        let context = self.context_definition();
        self.collected.clear();
        for occurrence in self.selected_assemblies() {
            self.queue.push_back((Operation::Explode { context, occurrence }, After::Collect));
        }
        self.pump();
    }

    /// The bodies selected in the open part, with its path.
    fn selected_bodies(&self) -> Option<(EntityPath, Vec<Id>)> {
        let target = self.open_part_path()?;
        let (_, p, _) = self.document().resolve_container(&target).ok()?;
        let bodies: Vec<Id> = self.selection.iter().copied().filter(|id| p.body(*id).is_some()).collect();
        Some((target, bodies))
    }

    /// Can the selection be grouped: bodies of the open part (docs/model/bodies.md#body-groups)?
    pub fn can_group_bodies(&self) -> bool {
        self.selected_bodies().is_some_and(|(_, b)| !b.is_empty())
    }

    /// Can the selection be ungrouped: a group among the bodies selected in the open part?
    pub fn can_ungroup_bodies(&self) -> bool {
        let Some((target, bodies)) = self.selected_bodies() else { return false };
        let Ok((_, p, _)) = self.document().resolve_container(&target) else { return false };
        bodies.iter().any(|id| p.body(*id).is_some_and(|b| p.def_of(b).is_group()))
    }

    /// Groups the bodies selected in the open part; the group then selected.
    pub fn group_bodies(&mut self) {
        if self.busy() {
            return;
        }
        let Some((target, bodies)) = self.selected_bodies().filter(|(_, b)| !b.is_empty()) else {
            self.message = Some(t!("status.nothing_to_group"));
            return;
        };
        let _ = self.start(Operation::GroupBodies { target, bodies, scope: self.part_scope }, After::SelectNamed, false);
    }

    /// Flattens the groups selected in the open part; what they held then selected.
    pub fn ungroup_bodies(&mut self) {
        if self.busy() {
            return;
        }
        let Some((target, bodies)) = self.selected_bodies().filter(|(_, b)| !b.is_empty()) else { return };
        let _ = self.start(Operation::UngroupBodies { target, bodies, scope: self.part_scope }, After::Select, false);
    }

    pub fn make_unique(&mut self) {
        if self.busy() {
            return;
        }
        self.made_unique.clear();
        // In an open part: its bodies selected, each given its own definition (docs/model/bodies.md).
        if let Some(target) = self.open_part_path() {
            let Ok((_, p, _)) = self.document().resolve_container(&target) else { return };
            let bodies: Vec<Id> = self.selection.iter().copied().filter(|id| p.body(*id).is_some()).collect();
            if !bodies.is_empty() {
                let op = Operation::MakeBodiesUnique { target, bodies, scope: self.part_scope };
                self.queue.push_back((op, After::Unique));
                self.pump();
            }
            return;
        }
        let context = self.context_definition();
        let Ok(a) = self.document().assembly(context) else { return };
        let occurrences: Vec<Id> = self.selection.iter().copied().filter(|id| a.occurrence(*id).is_some()).collect();
        for occurrence in occurrences {
            self.queue.push_back((Operation::MakeUnique { context, occurrence }, After::Unique));
        }
        self.pump();
    }
}

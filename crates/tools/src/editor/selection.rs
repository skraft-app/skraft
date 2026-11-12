//! Selection and the context being edited: select, enter and leave assemblies, open and close
//! a part (ARCHITECTURE.md §2.9), erase, make an assembly, explode, make a part on its own.

use solidsketch_i18n::t;
use solidsketch_model::{EntityPath, Id, Operation};

use super::{After, Editor};

impl Editor {
    pub fn select(&mut self, ids: Vec<Id>) {
        self.selection = ids;
    }

    pub fn select_all(&mut self) {
        let guides = self.shown_guides();
        if self.open_part.is_some() {
            self.selection = self
                .scene
                .recipe()
                .filter(|_| self.showing_machinings())
                .map(|r| r.tools.iter().map(|t| t.machining).collect())
                .unwrap_or_default();
            self.selection.extend(guides);
            return;
        }
        let ctx = self.context_definition();
        self.selection = self.document().assembly(ctx).map(|a| a.occurrences().iter().map(|o| o.id).collect()).unwrap_or_default();
        self.selection.extend(guides);
    }

    /// The component guides are drawn in now (§6 quinquies): the open part's, else the
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
            self.sync_scene();
        }
        self.selection = vec![path.entity];
    }

    /// Edits the assembly the occurrence `id` of the active context places, or opens the part
    /// it places.
    pub fn enter(&mut self, id: Id) {
        if self.open_part.is_some() {
            return;
        }
        let doc = self.document();
        let ctx = self.context_definition();
        let is_part = doc.assembly(ctx).ok().and_then(|a| a.occurrence(id)).is_some_and(|o| doc.part(o.component).is_ok());
        if doc.is_assembly_occurrence(ctx, id) {
            self.context.push(id);
        } else if is_part {
            self.open_part = Some(id);
            self.part_scope = solidsketch_model::Scope::All;
        } else {
            return;
        }
        self.tool.reset();
        self.selection.clear();
        self.listed_hover = None;
        self.sync_scene();
    }

    /// Closes the open part, else goes back to the parent context; the occurrence left is
    /// selected.
    pub fn exit(&mut self) {
        if let Some(id) = self.open_part.take().or_else(|| self.context.pop()) {
            self.tool.reset();
            self.selection = vec![id];
            self.listed_hover = None;
            self.sync_scene();
        }
    }

    pub fn delete_selection(&mut self) {
        if self.selection.is_empty() || self.busy() {
            return;
        }
        let entities = std::mem::take(&mut self.selection);
        self.erase(entities);
    }

    /// Erases entities of the active context: occurrences and guides, or, in an open part,
    /// its machinings and guides.
    fn erase(&mut self, entities: Vec<Id>) {
        let op = match self.open_part_path() {
            Some(target) => Operation::DeleteMachinings { target, machinings: entities, scope: self.part_scope },
            None => Operation::Delete { context: self.context_definition(), entities },
        };
        let _ = self.start(op, After::Nothing, false);
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

    /// Puts the selection in a new assembly.
    pub fn make_assembly(&mut self) {
        if self.busy() {
            return;
        }
        if self.selection.is_empty() {
            self.message = Some(t!("status.nothing_to_group"));
            return;
        }
        let op = Operation::MakeAssembly { context: self.context_definition(), entities: self.selection.clone(), name: None };
        let _ = self.start(op, After::Select, false);
    }

    /// The selected occurrences of assemblies, in the active context.
    pub(super) fn selected_assemblies(&self) -> Vec<Id> {
        let ctx = self.context_definition();
        self.selection.iter().copied().filter(|id| self.document().is_assembly_occurrence(ctx, *id)).collect()
    }

    pub fn explode(&mut self) {
        if self.busy() {
            return;
        }
        let context = self.context_definition();
        self.collected.clear();
        for occurrence in self.selected_assemblies() {
            self.queue.push_back((Operation::Explode { context, occurrence }, After::Collect));
        }
        self.pump();
    }

    pub fn make_unique(&mut self) {
        if self.busy() {
            return;
        }
        let context = self.context_definition();
        let Ok(a) = self.document().assembly(context) else { return };
        let occurrences: Vec<Id> = self.selection.iter().copied().filter(|id| a.occurrence(*id).is_some()).collect();
        for occurrence in occurrences {
            self.queue.push_back((Operation::MakeUnique { context, occurrence }, After::Nothing));
        }
        self.pump();
    }
}

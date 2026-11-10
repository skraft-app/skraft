//! Selection and the context being edited: select, enter and leave assemblies, erase, make
//! an assembly, explode, make a part on its own.

use solidsketch_i18n::t;
use solidsketch_model::{EntityPath, Id, Operation};

use super::{After, Editor};

impl Editor {
    pub fn select(&mut self, ids: Vec<Id>) {
        self.selection = ids;
    }

    pub fn select_all(&mut self) {
        let ctx = self.context_definition();
        self.selection = self.document().assembly(ctx).map(|a| a.occurrences.iter().map(|o| o.id).collect()).unwrap_or_default();
    }

    /// Selects the entity `path` names, editing the context it is in (the outliner's click).
    pub fn select_path(&mut self, path: &EntityPath) {
        if self.document().resolve(path).is_err() {
            return;
        }
        if self.context != path.instances {
            self.tool.reset();
            self.context = path.instances.clone();
        }
        self.selection = vec![path.entity];
    }

    /// Edits the assembly the occurrence `id` of the active context places.
    pub fn enter(&mut self, id: Id) {
        if self.document().is_assembly_occurrence(self.context_definition(), id) {
            self.tool.reset();
            self.context.push(id);
            self.selection.clear();
        }
    }

    /// Goes back to the parent context, the assembly's occurrence left selected.
    pub fn exit(&mut self) {
        if let Some(id) = self.context.pop() {
            self.tool.reset();
            self.selection = vec![id];
        }
    }

    pub fn delete_selection(&mut self) {
        if self.selection.is_empty() || self.busy() {
            return;
        }
        let entities = std::mem::take(&mut self.selection);
        let _ = self.start(Operation::Delete { context: self.context_definition(), entities }, After::Nothing, false);
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
        for occurrence in self.selection.clone() {
            self.queue.push_back((Operation::MakeUnique { context, occurrence }, After::Nothing));
        }
        self.pump();
    }
}

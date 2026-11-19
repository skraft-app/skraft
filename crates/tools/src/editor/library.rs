//! The document's components library (docs/ui/components-library.md): a component chosen in it,
//! its occurrences selected, the selection replaced by it, deleted, the unused ones purged.

use glam::{DAffine3, DVec3};
use skraft_i18n::t;
use skraft_model::{Document, EntityPath, Id, Operation};

use super::{After, Editor};
use crate::tools::{self, Moved, OptionValue};

impl Editor {
    /// The component chosen in the library (its entity info shown), while nothing else is:
    /// nothing selected — but the occurrence of it carried at the cursor —, no part open, the
    /// component still there.
    pub fn chosen_component(&self) -> Option<Id> {
        self.chosen.filter(|_| self.open_part.is_none() && (!self.anything_selected() || self.carry.is_some()))
    }

    /// Can `component` be placed from the library now? Not while a part is open, nor the
    /// assembly edited or one above it (a cycle), nor the model.
    pub fn can_place_component(&self, component: Id) -> bool {
        !self.library_disabled()
            && !self.busy()
            && component != self.document().root()
            && self.document().component(component).is_ok()
            && !self.on_active_path(component)
    }

    /// Places one more occurrence of `component` at the cursor (docs/ui/components-library.md#placing):
    /// placed in the active context at its origin, oriented as the context, then carried by
    /// the Move tool from the origin of its axes, as a paste at the cursor — a click puts it
    /// down (one step), Esc, Undo or another tool give it up. Chosen meanwhile (the entity info
    /// shows it). Says whether it is carried.
    pub fn place_component(&mut self, component: Id) -> bool {
        if !self.can_place_component(component) {
            return false;
        }
        // A carry under way is given up first.
        self.settle_carry(true);
        self.tool.reset();
        self.clear_selection();
        self.model_selected = false;
        let op = Operation::PlaceOccurrence { context: self.context_definition(), component, transform: DAffine3::IDENTITY };
        let from = self.context_transform().transform_point3(DVec3::ZERO);
        let placed = self.start(op, After::Carry(from), false).is_ok() && self.carrying();
        if placed {
            self.chosen = Some(component);
        }
        placed
    }

    /// Chooses `component` in the library: the selection let go, the model no more chosen; its
    /// entity info shown until anything else is chosen. Not while a part is open (the library
    /// is disabled then).
    pub fn choose_component(&mut self, component: Id) {
        if self.open_part.is_some() || component == self.document().root() || self.document().component(component).is_err() {
            return;
        }
        self.clear_selection();
        self.model_selected = false;
        self.chosen = Some(component);
    }

    /// Is the library disabled (a part open: its bodies come from bodies, not occurrences)?
    pub fn library_disabled(&self) -> bool {
        self.open_part.is_some()
    }

    /// Is `component` the assembly edited or one above it, along the active path (placing it
    /// would make a cycle, deleting it would take the context away)?
    pub fn on_active_path(&self, component: Id) -> bool {
        let doc = self.document();
        (0..=self.context.len()).any(|k| doc.context_of(&self.context[..k]).is_ok_and(|c| c == component))
    }

    /// The occurrences of `component` under the active context, at any depth, by their paths
    /// from the root.
    pub fn occurrences_under_context(&self, component: Id) -> Vec<EntityPath> {
        fn walk(doc: &Document, assembly: Id, target: Id, prefix: &mut Vec<Id>, out: &mut Vec<EntityPath>, depth: usize) {
            let Ok(a) = doc.assembly(assembly) else { return };
            if depth > skraft_model::MAX_DEPTH {
                return;
            }
            for o in a.occurrences() {
                if o.component == target {
                    out.push(EntityPath::new(prefix.clone(), o.id));
                } else if doc.assembly(o.component).is_ok() && doc.contains_component(o.component, target) {
                    prefix.push(o.id);
                    walk(doc, o.component, target, prefix, out, depth + 1);
                    prefix.pop();
                }
            }
        }
        let mut out = Vec::new();
        walk(self.document(), self.context_definition(), component, &mut self.context.clone(), &mut out, 0);
        out
    }

    /// « Sélectionner les occurrences »: the occurrences of `component` under the active
    /// context selected; the scope turned to « Pièces » when one lies deeper than the
    /// context's children (docs/tools/selection.md#selection-scope).
    pub fn select_occurrences_of(&mut self, component: Id) {
        if self.library_disabled() {
            return;
        }
        let paths = self.occurrences_under_context(component);
        if paths.is_empty() {
            self.message = Some(t!("status.no_occurrence_here"));
            return;
        }
        self.tool.reset();
        if paths.iter().all(|p| p.instances == self.context) {
            self.set_selection(paths.into_iter().map(|p| p.entity).collect());
        } else {
            if !self.pick_parts() {
                let parts = OptionValue::Choice(tools::PICK_PARTS.into());
                self.tool_settings.entry(tools::SELECTION_SETTINGS.to_owned()).or_default().insert(tools::PICK_SCOPE.to_owned(), parts);
            }
            self.select_paths(paths);
        }
        self.rehover();
    }

    /// Can the selection be replaced by occurrences of `component` (« Remplacer la
    /// sélection »)? Occurrences selected, not all of it already, none of the assemblies it
    /// is in (a cycle), no part open.
    pub fn can_replace_selection(&self, component: Id) -> bool {
        if self.library_disabled() || self.on_active_path(component) {
            return false;
        }
        let doc = self.document();
        let ctx = self.context_definition();
        let placed: Vec<(Id, Id)> = if self.deep.is_empty() {
            let Ok(a) = doc.assembly(ctx) else { return false };
            self.selection.iter().filter_map(|id| a.occurrence(*id)).map(|o| (ctx, o.component)).collect()
        } else {
            self.deep.iter().filter_map(|p| Some((doc.context_of(&p.instances).ok()?, doc.resolve(p).ok()?.component))).collect()
        };
        !placed.is_empty()
            && placed.iter().any(|(_, c)| *c != component)
            && placed.iter().all(|(parent, _)| !doc.contains_component(component, *parent))
    }

    /// « Remplacer la sélection »: the occurrences selected become occurrences of `component`,
    /// placed as they were (docs/ui/components-library.md#managing); at any depth, at each
    /// parent, made unique first as the scope says.
    pub fn replace_selection(&mut self, component: Id) {
        if self.busy() || !self.can_replace_selection(component) {
            return;
        }
        let op = if self.deep.is_empty() {
            Operation::ReplaceOccurrences { context: self.context_definition(), entities: self.selection.clone(), component }
        } else {
            let Moved::Deep { groups, unique } = self.cx().moved_deep(&self.deep, &[]) else { return };
            Operation::Batch(
                groups
                    .into_iter()
                    .map(|g| {
                        let op = Operation::ReplaceOccurrences { context: Id(0), entities: g.entities, component };
                        Operation::At { parent: g.parent, unique, op: Box::new(op) }
                    })
                    .collect(),
            )
        };
        let _ = self.start(op, After::Nothing, false);
    }

    /// Can `component` be deleted, with all its occurrences? Not the assembly edited nor one
    /// above it, nor while a part is open.
    pub fn can_delete_component(&self, component: Id) -> bool {
        !self.library_disabled() && component != self.document().root() && !self.on_active_path(component)
    }

    /// « Supprimer »: `component` and all its occurrences (docs/ui/components-library.md#managing).
    pub fn delete_component(&mut self, component: Id) {
        if self.busy() || !self.can_delete_component(component) {
            return;
        }
        self.tool.reset();
        self.clear_selection();
        let _ = self.start(Operation::DeleteComponent { component }, After::Nothing, false);
    }

    /// The components not in the model (« Inutilisés »): kept by a name, placed nowhere.
    pub fn unused_components(&self) -> Vec<Id> {
        let doc = self.document();
        let counts = doc.counts_in_model();
        doc.components().filter(|c| !counts.contains_key(&c.id)).map(|c| c.id).collect()
    }

    /// « Purger les inutilisés »: every component not in the model dropped.
    pub fn purge_unused(&mut self) {
        if self.busy() || self.library_disabled() || self.unused_components().is_empty() {
            return;
        }
        let _ = self.start(Operation::PurgeUnused, After::Nothing, false);
    }
}

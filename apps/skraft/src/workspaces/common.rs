//! What every workspace may show (docs/ui/workspaces.md): the document's cards (navigator,
//! entity info, components, materials), the boxes they open, the 3D view and what floats over
//! it. A workspace depends on this and on the app's layer, never on another workspace.

pub(crate) mod cards;
pub(crate) mod dialogs;
pub(crate) mod occurrence_name;
pub(crate) mod viewport;

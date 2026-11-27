//! The parts every screen builds with, knowing nothing of the document
//! (docs/ui/interface.md#widgets): the style and its tokens, the icons and cursors, the
//! widgets, the shapes drawn over the 2D stages, the search field, the handles the control
//! protocol clicks.

pub(crate) mod cursors;
pub(crate) mod handles;
pub(crate) mod icons;
pub(crate) mod list_select;
pub(crate) mod overlay;
pub(crate) mod search;
pub(crate) mod style;
pub(crate) mod touch;
pub(crate) mod widgets;

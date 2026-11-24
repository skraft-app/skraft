//! The name of an instance — an occurrence, a body —, written the same wherever it shows (the
//! navigator's rows, the tree drop-up, docs/tools/selection.md): its own name, if it
//! has one, in semibold, then its definition's between angle brackets (« Gauche <Pied> »,
//! « <Pied> »). The brackets faint, well apart from the names; the definition's name, automatic
//! (docs/model/data-model.md, docs/model/bodies.md#automatic-names), in italics and greyed.

use egui::Color32;
use egui::text::{LayoutJob, TextFormat};

/// How a name is inked.
#[derive(Clone, Copy)]
pub struct NameInk {
    /// The text's size.
    pub size: f32,
    /// The names'.
    pub ink: Color32,
    /// An automatic definition name's (the brackets', fainter still: [`FAINT`]).
    pub weak: Color32,
}

/// How much of the weak ink the brackets keep (the ribbon's unpinned pin's).
const FAINT: f32 = 0.55;

/// The name of an instance whose own name is `own` (empty: none) and whose definition is
/// named `def`, automatically when `auto`.
pub fn job(own: &str, def: &str, auto: bool, ink: NameInk) -> LayoutJob {
    let regular = |color: Color32| TextFormat { font_id: egui::FontId::proportional(ink.size), color, ..Default::default() };
    let faint = ink.weak.gamma_multiply(FAINT);
    let mut job = LayoutJob::default();
    if !own.is_empty() {
        job.append(own, 0.0, TextFormat { font_id: crate::kit::style::semibold(ink.size), ..regular(ink.ink) });
        job.append(" ", 0.0, regular(ink.ink));
    }
    job.append("<", 0.0, regular(faint));
    let def_format = if auto { TextFormat { italics: true, ..regular(ink.weak) } } else { regular(ink.ink) };
    job.append(def, 0.0, def_format);
    job.append(">", 0.0, regular(faint));
    job
}

/// Where, from the name's left, its own name `own` ends and its definition's starts: halfway
/// across the space between them (a rename edits the one under the pointer).
pub fn split(ui: &egui::Ui, own: &str, size: f32) -> f32 {
    let font = crate::kit::style::semibold(size);
    let width = |t: String| ui.fonts_mut(|f| f.layout_no_wrap(t, font.clone(), Color32::WHITE).size().x);
    let space = ui.fonts_mut(|f| f.layout_no_wrap(" ".into(), egui::FontId::proportional(size), Color32::WHITE).size().x);
    width(own.to_owned()) + space / 2.0
}

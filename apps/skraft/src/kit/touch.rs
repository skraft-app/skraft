//! Two fingers on a stage (docs/ui/touch.md#gestures-in-the-view): they pan and pinch it, and
//! what the stage's tools do with the pointer is hidden from the second finger down until the
//! last is lifted. The platforms emulate a mouse with the first finger (a primary button pressed,
//! moved, released): without this, a pinch would end in a click.

/// This frame's two-finger gesture.
#[derive(Clone, Copy, Debug)]
pub struct Pinch {
    /// Its first frame: the stage sets its pivot.
    pub began: bool,
    /// The fingers' middle, and how far it moved (points).
    pub center: egui::Pos2,
    pub translation: egui::Vec2,
    /// The fingers' spread, now over before (> 1: apart).
    pub zoom: f32,
}

/// Whether a touch on a stage navigates, kept from frame to frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct TwoFingers {
    active: bool,
}

impl TwoFingers {
    /// Read before the stage's widgets: whether the pointer is the gesture's this frame — the
    /// stage's tools then get nothing of it, the last finger's release included — and the
    /// gesture's moves, when two fingers or more are down. A gesture is the stage's when it began
    /// over `stage`, on its layer (not over a card floating on it).
    pub fn update(&mut self, ui: &egui::Ui, stage: egui::Rect) -> (bool, Option<Pinch>) {
        let ctx = ui.ctx();
        let (touching, down, multi) = ctx.input(|i| (i.any_touches(), i.pointer.any_down(), i.multi_touch()));
        let ours = |p: egui::Pos2| stage.contains(p) && ctx.layer_id_at(p) == Some(ui.layer_id());
        if let Some(m) = multi.filter(|m| self.active || ours(m.start_pos)) {
            let began = !self.active;
            self.active = true;
            return (true, Some(Pinch { began, center: m.center_pos, translation: m.translation_delta, zoom: m.zoom_delta }));
        }
        let hidden = self.active;
        // One finger of two lifted: nothing more till they all are.
        if !touching && !down {
            self.active = false;
        }
        (hidden, None)
    }
}

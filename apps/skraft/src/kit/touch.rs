//! Two fingers on a stage (docs/ui/touch.md#gestures-in-the-view): they pan and pinch it, and
//! what the stage's tools do with the pointer is hidden from the second finger down until the
//! last is lifted. The platforms emulate a mouse with the first finger (a primary button pressed,
//! moved, released): without this, a pinch would end in a click.
//!
//! Lists under a finger (docs/ui/touch.md#lists): a finger dragged scrolls them, and what they
//! hold is picked up by a long press, then moved.

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

/// The key, in the context's memory, of whether the pointer is a finger.
const FINGER: &str = "pointer_is_finger";

/// Read at the start of a frame: whether the pointer is a finger — its last events came with a
/// touch's — or a mouse; `coarse` (the screen's pointer) until it has moved.
pub fn watch_pointer(ctx: &egui::Context, coarse: bool) {
    let (touch, pointer) = ctx.input(|i| {
        let touch = i.events.iter().any(|e| matches!(e, egui::Event::Touch { .. }));
        let pointer = i.events.iter().any(|e| matches!(e, egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. }));
        (touch, pointer)
    });
    ctx.data_mut(|d| {
        let finger = d.get_temp_mut_or(egui::Id::unique(FINGER), coarse);
        if touch {
            *finger = true;
        } else if pointer {
            *finger = false;
        }
    });
}

/// Is the pointer a finger ([`watch_pointer`])?
pub fn finger(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp(egui::Id::unique(FINGER))).unwrap_or(false)
}

/// How a list in a card scrolls: under a finger, dragged from the first touch on (egui's
/// default waits to have seen a touch screen); under a mouse, as egui's.
pub fn list_scroll(ctx: &egui::Context) -> egui::scroll_area::ScrollSource {
    if finger(ctx) { egui::scroll_area::ScrollSource::ALL } else { egui::scroll_area::ScrollSource::default() }
}

/// The key, in the context's memory, of the element long-pressed: its id, the press's start
/// (an element long-pressed by an earlier touch is not this one's) and where it was.
const PRESSED: &str = "list_long_pressed";

/// An element long-pressed ([`drag_source`]).
#[derive(Clone, Copy)]
struct Pressed {
    id: egui::Id,
    since: f64,
    at: egui::Pos2,
}

/// An element of a list that can be dragged (a row, a tile): `response` given back, sensing
/// drags under a mouse, and whether it was picked up this frame. Under a mouse, when a drag
/// starts; under a finger — a drag scrolling the list —, when the finger, long-pressed on it
/// (its menu open), moves on: the menu closes and the element goes with the finger. What is
/// dragged follows the pointer, as a mouse's drag does, till it is lifted.
pub fn drag_source(ui: &egui::Ui, response: egui::Response) -> (egui::Response, bool) {
    let ctx = ui.ctx();
    if !finger(ctx) {
        let response = response.interact(egui::Sense::click_and_drag());
        let lifted = response.drag_started_by(egui::PointerButton::Primary);
        return (response, lifted);
    }
    let key = egui::Id::unique(PRESSED);
    let (down, since, pos) = ctx.input(|i| (i.pointer.primary_down(), i.pointer.press_start_time(), i.pointer.interact_pos()));
    if response.long_touched()
        && let (Some(since), Some(at)) = (since, pos)
    {
        ctx.data_mut(|d| d.insert_temp(key, Pressed { id: response.id, since, at }));
        return (response, false);
    }
    let pressed = ctx.data(|d| d.get_temp::<Pressed>(key)).filter(|p| p.id == response.id);
    let Some(pressed) = pressed.filter(|p| down && since == Some(p.since)) else {
        if pressed.is_some() {
            ctx.data_mut(|d| d.remove::<Pressed>(key));
        }
        return (response, false);
    };
    let moved = pos.is_some_and(|p| p.distance(pressed.at) > crate::window::form::TAP_SLOP);
    if moved {
        ctx.data_mut(|d| d.remove::<Pressed>(key));
        egui::Popup::close_all(ctx);
    }
    (response, moved)
}

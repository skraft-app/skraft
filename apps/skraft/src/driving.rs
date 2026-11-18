//! The app's side of the control protocol (docs/ARCHITECTURE.md §7 septies), whatever carries
//! the requests — the desktop's loopback server, the page's `window.skraft`: who asked
//! ([`Asker`]), what waits for a later frame (`app.wait`, a real input taken in, a window's
//! screenshot), the real egui events fed one frame at a time (`ui.*`), the status bar's
//! messages.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use skraft_control::params::{UiButton, UiMods, WaitParams};
use skraft_control::{Call, Outcome};
use skraft_tools::journal::Origin;
use web_time::Instant;

/// A request and where its answer goes.
pub struct Asker {
    pub call: Call,
    reply: Box<dyn FnOnce(Outcome)>,
}

impl Asker {
    pub fn new(call: Call, reply: impl FnOnce(Outcome) + 'static) -> Self {
        Asker { call, reply: Box::new(reply) }
    }

    pub fn answer(self, outcome: Outcome) {
        (self.reply)(outcome)
    }
}

/// An `app.wait` not met yet: answered at the frame that meets it, or at its deadline.
pub struct Waiting {
    pub asker: Asker,
    pub params: WaitParams,
    pub deadline: Instant,
}

/// The events of one frame, the modifiers held during it, the client that asked for them.
struct Frame {
    events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
    client: Option<String>,
}

/// A `ui.*` request whose events are queued: answered once the frame after its last one has
/// begun (its events then taken in by the widgets).
struct Taking {
    asker: Asker,
    /// Its last frame's number among those queued.
    ticket: u64,
    /// The frame its last events were fed to.
    fed_at: Option<u64>,
}

/// Whether a window event is a person's input (the pointer, the keyboard, the wheel), not
/// the window's own (focus, a screenshot).
fn is_input(e: &egui::Event) -> bool {
    use egui::Event as E;
    matches!(
        e,
        E::PointerMoved(_)
            | E::MouseMoved(_)
            | E::PointerButton { .. }
            | E::PointerGone
            | E::Key { .. }
            | E::Text(_)
            | E::MouseWheel { .. }
            | E::Zoom(_)
            | E::Rotate(_)
            | E::Touch { .. }
            | E::Copy
            | E::Cut
            | E::Paste(_)
            | E::Ime(_)
    )
}

/// The messages kept.
const MESSAGES: usize = 200;

/// A message of the status bar, when it was first shown (ms since the app started).
pub struct Message {
    pub text: String,
    pub ms: f64,
}

pub struct Driving {
    start: Instant,
    /// The frame running (counted from 1 by [`Driving::frame_start`]).
    pub frame: u64,
    pub waiting: Vec<Waiting>,
    queue: VecDeque<Frame>,
    queued: u64,
    fed: u64,
    /// The modifiers the queued frames hold now.
    held: egui::Modifiers,
    /// Who caused this frame's input: the client whose events were fed to it, the user if
    /// they gave any, else whoever did last (a frame with no input — a tool following the
    /// pointer where the pilot left it — goes on with the last one's).
    origin: Origin,
    taking: Vec<Taking>,
    /// The window's screenshot asked for, and the image once there.
    shot: Vec<Asker>,
    image: Arc<Mutex<Option<Arc<egui::ColorImage>>>>,
    pub messages: VecDeque<Message>,
}

impl Default for Driving {
    fn default() -> Self {
        Driving {
            start: Instant::now(),
            frame: 0,
            waiting: Vec::new(),
            queue: VecDeque::new(),
            queued: 0,
            fed: 0,
            held: egui::Modifiers::NONE,
            origin: Origin::User,
            taking: Vec::new(),
            shot: Vec::new(),
            image: Arc::new(Mutex::new(None)),
            messages: VecDeque::new(),
        }
    }
}

impl Driving {
    pub fn frame_start(&mut self) {
        self.frame += 1;
    }

    /// Is anything waiting for a frame (frames must follow)?
    pub fn busy(&self) -> bool {
        !self.queue.is_empty() || !self.taking.is_empty() || !self.waiting.is_empty() || !self.shot.is_empty()
    }

    /// Who caused this frame's input (the journal's entries): a client of the channel whose
    /// real events were fed to it, else the user.
    pub fn origin(&self) -> Origin {
        self.origin.clone()
    }

    /// The user acted other than through the window's events (the native menu bar, a file
    /// the system opens): what follows is theirs.
    pub fn user_acted(&mut self) {
        self.origin = Origin::User;
    }

    /// Queues `frames` of events for `asker`, answered once taken in.
    pub fn take_in(&mut self, asker: Asker, frames: Vec<(Vec<egui::Event>, egui::Modifiers)>) {
        for (events, modifiers) in frames {
            self.queue.push_back(Frame { events, modifiers, client: asker.call.client.clone() });
            self.queued += 1;
        }
        self.taking.push(Taking { asker, ticket: self.queued, fed_at: None });
    }

    /// Feeds the next frame's events to egui (`raw_input_hook`): the next queued frame, its
    /// modifiers pressed first when they change (egui follows them by events), let go once
    /// the queue is empty.
    pub fn feed(&mut self, raw: &mut egui::RawInput) {
        if raw.events.iter().any(is_input) {
            self.origin = Origin::User;
        }
        let Some(f) = self.queue.pop_front() else {
            if self.held != egui::Modifiers::NONE {
                self.held = egui::Modifiers::NONE;
                raw.events.push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
            }
            return;
        };
        if f.modifiers != self.held {
            self.held = f.modifiers;
            raw.events.push(egui::Event::ModifiersChanged(f.modifiers));
        }
        raw.events.extend(f.events);
        self.origin = Origin::control(f.client.as_deref());
        self.fed += 1;
        // The frame about to run takes them in.
        let next = self.frame + 1;
        for t in self.taking.iter_mut().filter(|t| t.fed_at.is_none() && t.ticket <= self.fed) {
            t.fed_at = Some(next);
        }
    }

    /// The requests whose events a frame before this one took in: to answer now.
    pub fn taken(&mut self) -> Vec<Asker> {
        let frame = self.frame;
        let (done, left): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.taking).into_iter().partition(|t| t.fed_at.is_some_and(|f| f < frame));
        self.taking = left;
        done.into_iter().map(|t| t.asker).collect()
    }

    /// Asks egui for the window's image, for `asker`.
    pub fn screenshot(&mut self, ctx: &egui::Context, asker: Asker) {
        if self.shot.is_empty() {
            let slot = self.image.clone();
            let ctx2 = ctx.clone();
            ctx.request_screenshot(move |image| {
                if let Ok(mut s) = slot.lock() {
                    *s = Some(image);
                }
                ctx2.request_repaint();
            });
        }
        self.shot.push(asker);
    }

    /// The window's image, once it came, and who asked for it.
    pub fn screenshot_taken(&mut self) -> Option<(Arc<egui::ColorImage>, Vec<Asker>)> {
        let image = self.image.lock().ok()?.take()?;
        Some((image, std::mem::take(&mut self.shot)))
    }

    /// The status bar shows `text`: kept when it changed.
    pub fn message(&mut self, text: &str) {
        if text.is_empty() || self.messages.back().is_some_and(|m| m.text == text) {
            return;
        }
        if self.messages.len() == MESSAGES {
            self.messages.pop_front();
        }
        self.messages.push_back(Message { text: text.to_owned(), ms: self.start.elapsed().as_secs_f64() * 1000.0 });
    }
}

pub fn modifiers(m: &UiMods) -> egui::Modifiers {
    egui::Modifiers {
        alt: m.alt,
        ctrl: m.ctrl,
        shift: m.shift,
        mac_cmd: m.command && cfg!(target_os = "macos"),
        command: m.command || m.ctrl,
    }
}

pub fn button(b: UiButton) -> egui::PointerButton {
    match b {
        UiButton::Left => egui::PointerButton::Primary,
        UiButton::Right => egui::PointerButton::Secondary,
        UiButton::Middle => egui::PointerButton::Middle,
        UiButton::Back => egui::PointerButton::Extra1,
        UiButton::Forward => egui::PointerButton::Extra2,
    }
}

/// A click's frames at `pos`: the pointer moved there, pressed, let go (twice for a double).
pub fn click(pos: egui::Pos2, b: UiButton, double: bool, mods: egui::Modifiers) -> Vec<(Vec<egui::Event>, egui::Modifiers)> {
    let press = |pressed| egui::Event::PointerButton { pos, button: button(b), pressed, modifiers: mods };
    let mut frames = vec![(vec![egui::Event::PointerMoved(pos)], mods)];
    for _ in 0..if double { 2 } else { 1 } {
        frames.push((vec![press(true)], mods));
        frames.push((vec![press(false)], mods));
    }
    frames
}

/// A drag's frames: moved to `from`, pressed, moved in `steps` to `to`, let go.
pub fn drag(
    from: egui::Pos2,
    to: egui::Pos2,
    steps: usize,
    b: UiButton,
    mods: egui::Modifiers,
) -> Vec<(Vec<egui::Event>, egui::Modifiers)> {
    let press = |pos, pressed| egui::Event::PointerButton { pos, button: button(b), pressed, modifiers: mods };
    let mut frames = vec![(vec![egui::Event::PointerMoved(from)], mods), (vec![press(from, true)], mods)];
    for k in 1..=steps {
        frames.push((vec![egui::Event::PointerMoved(from.lerp(to, k as f32 / steps as f32))], mods));
    }
    frames.push((vec![press(to, false)], mods));
    frames
}

/// RGBA rows to a PNG.
pub fn png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, width, height);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().and_then(|mut w| w.write_image_data(rgba)).map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asker(answered: &std::rc::Rc<std::cell::Cell<bool>>) -> Asker {
        let a = answered.clone();
        Asker::new(Call::new("ui.click", serde_json::Value::Null), move |_| a.set(true))
    }

    #[test]
    fn a_request_is_answered_the_frame_after_its_last_events() {
        let mut d = Driving::default();
        let done = std::rc::Rc::new(std::cell::Cell::new(false));
        d.take_in(asker(&done), click(egui::pos2(5.0, 5.0), UiButton::Left, false, egui::Modifiers::NONE));
        let mut frames = 0;
        while d.busy() {
            let mut raw = egui::RawInput::default();
            d.feed(&mut raw);
            d.frame_start();
            for a in d.taken() {
                a.answer(Ok(skraft_control::Answer::Json(serde_json::Value::Null)));
            }
            frames += 1;
            assert!(frames < 10);
            // Not before its three frames went through.
            assert_eq!(done.get(), frames > 3, "frame {frames}");
        }
        assert_eq!(frames, 4);
    }

    #[test]
    fn a_screenshot_is_a_png() {
        let bytes = png(2, 1, &[255, 0, 0, 255, 0, 255, 0, 255]).unwrap();
        assert_eq!(&bytes[1..4], b"PNG");
    }

    #[test]
    fn messages_are_kept_when_they_change() {
        let mut d = Driving::default();
        d.message("a");
        d.message("a");
        d.message("");
        d.message("b");
        assert_eq!(d.messages.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    }
}

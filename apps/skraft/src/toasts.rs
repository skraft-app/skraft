//! Toasts (docs/ui/interface.md#toasts): the app's messages — a file not opened or not
//! saved, an operation the kernel failed, a file imported, « Enregistré »… — stacked at the
//! bottom center of the window, above the status bar, which keeps the tools' hints. As
//! OpenCutList's notifications (`SmartTool#notify`): a click closes one; an info or a success
//! goes by itself, a bar at its bottom emptying towards the left as it runs out (the pointer
//! over it stops it, full again); an error stays. A toast may offer an action (a button at its
//! right end, as OpenCutList's « Ouvrir » after an export), run by its click: « Renommer » on
//! « Nouvelle pièce créée » (docs/tools/quick-tools.md), gone with the part.
//! The same message again counts (×2).

use skraft_i18n::t;

use crate::docs::DocId;

/// What a toast says, and its colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// Neutral; goes by itself.
    Info,
    /// Green; goes by itself.
    Success,
    /// Amber; stays until clicked.
    Warning,
    /// Red; stays until clicked.
    Error,
}

impl Level {
    fn timed(self) -> bool {
        matches!(self, Level::Info | Level::Success)
    }

    fn name(self) -> &'static str {
        match self {
            Level::Info => "info",
            Level::Success => "success",
            Level::Warning => "warning",
            Level::Error => "error",
        }
    }
}

/// What a toast's button does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    /// Shows the file in the Finder, the Explorer or the file manager.
    // No toast offers it for now: kept, for those that will.
    #[allow(dead_code)]
    Reveal(String),
    /// Opens the file with the system's application for its kind (a PDF exported).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    Open(String),
    /// Opens the naming dialog on what was just made (a part, an assembly, a body definition),
    /// of this document.
    Rename { doc: DocId, target: skraft_tools::NameTarget },
    /// Acts on what an operation left of linked bodies, in this document
    /// (docs/decisions/0055-linked-bodies-through-imprints.md).
    Link { doc: DocId, act: skraft_tools::LinkAct },
    /// Opens the window of the new version (docs/ui/updates.md).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    ShowUpdate,
}

/// A message for the user: a title, and what went wrong or more about it (the system's or the
/// kernel's words, shown smaller); maybe a button.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub level: Level,
    pub title: String,
    pub detail: Option<String>,
    /// A button: its label, what it does.
    pub action: Option<(String, Act)>,
    /// A second button, after it (boxed: a note travels in `Result`s).
    pub also: Option<Box<(String, Act)>>,
}

impl Note {
    pub fn new(level: Level, title: impl Into<String>, detail: Option<String>) -> Note {
        Note { level, title: title.into(), detail, action: None, also: None }
    }

    pub fn info(title: impl Into<String>) -> Note {
        Note::new(Level::Info, title, None)
    }

    pub fn success(title: impl Into<String>) -> Note {
        Note::new(Level::Success, title, None)
    }

    pub fn error(title: impl Into<String>, detail: impl ToString) -> Note {
        Note::new(Level::Error, title, Some(detail.to_string()))
    }

    pub fn with_action(mut self, label: impl Into<String>, act: Act) -> Note {
        self.action = Some((label.into(), act));
        self
    }

    /// A notice of the editor of document `doc`, as a toast.
    pub fn of_notice(n: skraft_tools::Notice, doc: DocId) -> Note {
        use skraft_tools::NoticeKind as K;
        let level = match n.kind {
            K::Info => Level::Info,
            K::Success => Level::Success,
            K::Warning => Level::Warning,
            K::Error => Level::Error,
        };
        let button = |a: skraft_tools::NoticeAction| match a {
            skraft_tools::NoticeAction::Rename(target) => (t!("notice.rename"), Act::Rename { doc, target }),
            skraft_tools::NoticeAction::Link(act) => {
                let label = match act {
                    skraft_tools::LinkAct::Replace => t!("operation.reseat_sources"),
                    skraft_tools::LinkAct::Separate => t!("operation.separate_receivers"),
                };
                (label, Act::Link { doc, act })
            }
        };
        Note { action: n.action.map(button), also: n.also.map(|a| Box::new(button(a))), ..Note::new(level, n.title, n.detail) }
    }
}

/// On one line (the control channel's answers, the logs).
impl std::fmt::Display for Note {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.detail {
            Some(detail) => f.write_str(&t!("toast.with_detail", title = self.title, detail = detail)),
            None => f.write_str(&self.title),
        }
    }
}

/// How long an info or a success stays (seconds), as OpenCutList's.
const SECONDS: f64 = 5.0;
/// The most toasts at once: the oldest goes (an info or a success first).
const MAX: usize = 6;
/// A toast's width (points).
const WIDTH: f32 = 380.0;
/// The room under the toasts, above the status bar, and between them (points).
const GAP: f32 = 10.0;
/// The countdown bar's height (points).
const BAR: f32 = 1.0;
/// The corners of a toast and of its button (points).
const RADIUS: u8 = 3;
/// Between a toast's title and its detail (points).
const LINE_GAP: f32 = 3.0;

struct Toast {
    note: Note,
    /// Times shown (the same note again counts).
    count: u32,
    /// When its countdown started (egui's time), from its first frame on screen.
    since: Option<f64>,
    /// Where it was drawn last frame: its click is taken there before its content is drawn,
    /// so its button stays on top of it.
    rect: Option<egui::Rect>,
}

/// The toasts shown.
#[derive(Default)]
pub struct Toasts {
    list: Vec<Toast>,
}

impl Toasts {
    pub fn push(&mut self, note: Note) {
        log::info!("{note}");
        if let Some(t) = self.list.iter_mut().find(|t| t.note == note) {
            t.count += 1;
            t.since = None;
            return;
        }
        self.list.push(Toast { note, count: 1, since: None, rect: None });
        while self.list.len() > MAX {
            let i = self.list.iter().position(|t| t.note.level.timed()).unwrap_or(0);
            self.list.remove(i);
        }
    }

    /// Drops the toasts whose button would act on nothing any more (`live` false: a part
    /// undone or erased, its document closed).
    pub fn retain_live(&mut self, live: impl Fn(&Act) -> bool) {
        self.list.retain(|t| t.note.action.as_ref().is_none_or(|(_, act)| live(act)));
    }

    /// The toasts shown, oldest first: the control channel's `app.state`.
    pub fn json(&self) -> serde_json::Value {
        let list: Vec<_> = self
            .list
            .iter()
            .map(|t| {
                serde_json::json!({
                    "level": t.note.level.name(),
                    "title": t.note.title,
                    "detail": t.note.detail,
                    "action": t.note.action.as_ref().map(|(label, _)| label),
                    "also": t.note.also.as_ref().map(|b| &b.0),
                    "count": t.count,
                })
            })
            .collect();
        serde_json::Value::Array(list)
    }

    /// Draws the toasts, the newest at the bottom, just above `bottom` (the status bar's top),
    /// centered in the window. Returns the actions whose button was clicked.
    pub fn show(&mut self, ctx: &egui::Context, bottom: f32) -> Vec<Act> {
        let mut acts = Vec::new();
        if self.list.is_empty() {
            return acts;
        }
        let now = ctx.input(|i| i.time);
        let center = ctx.content_rect().center().x;
        let mut closed = Vec::new();
        egui::Area::new(egui::Id::unique("toasts"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::CENTER_BOTTOM)
            .fixed_pos(egui::pos2(center, bottom - GAP))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = GAP;
                let dark = ui.visuals().dark_mode;
                for (i, t) in self.list.iter_mut().enumerate() {
                    // The click on the toast, where it was: under its button, drawn after.
                    let id = egui::Id::unique(("toast", i));
                    let whole = t.rect.map(|r| ui.interact(r, id, egui::Sense::click()));
                    let (ink, ground, border) = colors(ui.visuals(), t.note.level, dark);
                    let body = ui.visuals().strong_text_color();
                    let frame = egui::Frame::popup(ui.style())
                        .fill(ground)
                        .stroke(egui::Stroke::new(1.0, border))
                        .corner_radius(RADIUS)
                        .inner_margin(egui::Margin { left: 12, right: 10, top: 8, bottom: 10 });
                    let r = frame.show(ui, |ui| {
                        ui.set_width(WIDTH);
                        // One row: the text, then its buttons at the right end, all centered on
                        // the row's height (the buttons' at least).
                        let buttons: Vec<_> = [(t.note.action.as_ref(), "action"), (t.note.also.as_deref(), "also")]
                            .into_iter()
                            .filter_map(|(button, handle)| {
                                let (label, act) = button?;
                                let label = ui.painter().layout_no_wrap(label.clone(), egui::TextStyle::Button.resolve(ui.style()), ink);
                                let size =
                                    (label.size() + 2.0 * ui.spacing().button_padding).max(egui::vec2(0.0, ui.spacing().interact_size.y));
                                Some((label, size, act.clone(), handle))
                            })
                            .collect();
                        let gap = 2.0 * ui.spacing().item_spacing.x;
                        // A warning's sign at the left, as high as a line of the title.
                        let sign = ui.text_style_height(&egui::TextStyle::Body);
                        let indent = if t.note.level == Level::Warning { sign + gap } else { 0.0 };
                        let room = WIDTH - indent - buttons.iter().map(|b| b.1.x + gap).sum::<f32>();
                        // The text laid out first, to center it on the row.
                        let title = if t.count > 1 { format!("{} (×{})", t.note.title, t.count) } else { t.note.title.clone() };
                        let title = ui.painter().layout(title, egui::TextStyle::Body.resolve(ui.style()), ink, room);
                        let detail = t
                            .note
                            .detail
                            .as_ref()
                            .map(|d| ui.painter().layout(d.clone(), egui::TextStyle::Small.resolve(ui.style()), body, room));
                        let text_h = title.size().y + detail.as_ref().map_or(0.0, |d| LINE_GAP + d.size().y);
                        let row_h = buttons.iter().fold(text_h, |h, b| h.max(b.1.y));
                        let (row, _) = ui.allocate_exact_size(egui::vec2(WIDTH, row_h), egui::Sense::hover());
                        let mut y = row.top() + (row_h - text_h) / 2.0;
                        if indent > 0.0 {
                            let at = egui::Rect::from_min_size(egui::pos2(row.left(), row.center().y - sign / 2.0), egui::vec2(sign, sign));
                            egui::Image::new(crate::icons::WARNING).tint(ink).paint_at(ui, at);
                        }
                        let left = row.left() + indent;
                        ui.painter().galley(egui::pos2(left, y), title.clone(), ink);
                        y += title.size().y + LINE_GAP;
                        if let Some(d) = detail {
                            ui.painter().galley(egui::pos2(left, y), d, body);
                        }
                        let mut right = row.right();
                        for (label, size, act, handle) in buttons.into_iter().rev() {
                            // Drawn here, not an egui button: egui's restyles a hovered button
                            // (margins, stroke, swelling) and the toast moved with it.
                            let at = egui::Rect::from_center_size(egui::pos2(right - size.x / 2.0, row.center().y), size);
                            right -= size.x + gap;
                            let b = ui.interact(at, egui::Id::unique(("toast.action", i, handle)), egui::Sense::click());
                            // Off the wash: the window's plain fill, framed and written in the
                            // toast's colour; tinted under the pointer.
                            let plain = ui.visuals().window_fill;
                            let fill = if b.hovered() { plain.lerp_to_gamma(border, 0.15) } else { plain };
                            let p = ui.painter();
                            p.rect(at, RADIUS, fill, egui::Stroke::new(1.0, border), egui::StrokeKind::Inside);
                            p.galley(at.center() - label.size() / 2.0, label, ink);
                            if b.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            crate::handles::publish(|| format!("toast.{i}.{handle}"), at);
                            if b.clicked() {
                                acts.push(act);
                                closed.push(i);
                            }
                        }
                    });
                    let rect = r.response.rect;
                    t.rect = Some(rect);
                    crate::handles::publish(|| format!("toast.{i}"), rect);
                    if whole.is_some_and(|w| w.clicked()) && !closed.contains(&i) {
                        closed.push(i);
                    }
                    if !t.note.level.timed() {
                        continue;
                    }
                    // The pointer over it stops its countdown, full again.
                    if ui.rect_contains_pointer(rect) {
                        t.since = Some(now);
                    }
                    let since = *t.since.get_or_insert(now);
                    let left = (1.0 - (now - since) / SECONDS).clamp(0.0, 1.0) as f32;
                    if left <= 0.0 && !closed.contains(&i) {
                        closed.push(i);
                    }
                    // The countdown: a bar along the bottom, emptying towards the left.
                    // Between the fill and the border: lighter than the border, darker than the
                    // fill (the other way round in a dark theme).
                    let inner = rect.shrink(3.0);
                    let bar = egui::Rect::from_min_max(
                        egui::pos2(inner.left(), inner.bottom() - BAR),
                        egui::pos2(inner.left() + inner.width() * left, inner.bottom()),
                    );
                    ui.painter().rect_filled(bar, 1.0, ground.lerp_to_gamma(border, 0.4));
                }
            });
        for i in closed.into_iter().rev() {
            self.list.remove(i);
        }
        // The countdowns move: the next frame soon (not when nothing runs).
        if self.list.iter().any(|t| t.note.level.timed()) {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        acts
    }
}

/// A toast's text colour, fill and border: its level's colour for the border and the title, a
/// pale wash of it for the fill (a dark one in a dark theme).
pub(crate) fn colors(v: &egui::Visuals, level: Level, dark: bool) -> (egui::Color32, egui::Color32, egui::Color32) {
    use egui::Color32 as C;
    let base = v.window_fill;
    let hue = match (level, dark) {
        (Level::Info, _) => return (v.strong_text_color(), base, v.window_stroke.color),
        (Level::Success, false) => C::from_rgb(46, 125, 50),
        (Level::Success, true) => C::from_rgb(102, 187, 106),
        (Level::Warning, false) => C::from_rgb(176, 120, 0),
        (Level::Warning, true) => C::from_rgb(255, 193, 7),
        (Level::Error, false) => C::from_rgb(198, 40, 40),
        (Level::Error, true) => C::from_rgb(239, 83, 80),
    };
    (hue, base.lerp_to_gamma(hue, if dark { 0.18 } else { 0.10 }), hue)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_note_counts_and_the_oldest_timed_one_goes_first() {
        let mut t = Toasts::default();
        t.push(Note::error("a", "x"));
        t.push(Note::success("b"));
        t.push(Note::error("a", "x"));
        assert_eq!(t.list.len(), 2);
        assert_eq!(t.list[0].count, 2);
        for i in 0..MAX {
            t.push(Note::error(format!("e{i}"), "y"));
        }
        // The success went first, then the oldest error.
        assert_eq!(t.list.len(), MAX);
        assert!(t.list.iter().all(|t| t.note.level == Level::Error));
        assert_eq!(t.list[0].note.title, "e0");
    }

    #[test]
    fn a_toast_may_offer_a_button() {
        let mut t = Toasts::default();
        t.push(Note::success("s").with_action("Ouvrir", Act::Reveal("/a.csv".into())));
        assert_eq!(t.json()[0]["action"], "Ouvrir");
    }

    #[test]
    fn a_notice_of_the_editor_is_a_toast() {
        use skraft_tools::{NameTarget, Notice, NoticeAction, NoticeKind};
        let n = Notice { kind: NoticeKind::Error, title: "t".into(), detail: Some("d".into()), action: None, also: None };
        assert_eq!(Note::of_notice(n, 1), Note::error("t", "d"));
        let target = NameTarget::Component(skraft_model::Id(7));
        let n =
            Notice { kind: NoticeKind::Success, title: "p".into(), detail: None, action: Some(NoticeAction::Rename(target)), also: None };
        let note = Note::of_notice(n, 2);
        assert_eq!(note.action, Some(("Renommer".into(), Act::Rename { doc: 2, target })));
        // Two buttons, about linked bodies.
        let (replace, separate) = (skraft_tools::LinkAct::Replace, skraft_tools::LinkAct::Separate);
        let links = Notice {
            kind: NoticeKind::Warning,
            title: "l".into(),
            detail: None,
            action: Some(NoticeAction::Link(replace)),
            also: Some(NoticeAction::Link(separate)),
        };
        let shown = Note::of_notice(links, 3);
        assert_eq!(shown.action, Some(("Replacer".into(), Act::Link { doc: 3, act: replace })));
        assert_eq!(shown.also, Some(Box::new(("Séparer".into(), Act::Link { doc: 3, act: separate }))));
        // Gone with its part.
        let mut t = Toasts::default();
        t.push(note);
        t.push(Note::info("i"));
        t.retain_live(|_| false);
        assert_eq!(t.list.len(), 1);
        assert_eq!(t.list[0].note.title, "i");
    }
}

//! The journal window (docs/control-protocol.md): what the shown document's editor
//! went through — its journal and the kernel's failures — as `journal.log` and
//! `journal.failures` give them to the control channel, read in the app. Opened from the
//! control channel's mark or the command palette (`app.journal`). What a client of the channel
//! caused (an agent, a script: the pilot) bears the channel's mark, its client's name on
//! hover; a filter shows the user's entries or the pilot's alone.

use skraft_i18n::{t, tp};
use skraft_tools::journal::{Entry, Event, Failure, JobKind, Journal, Origin};
use skraft_tools::{Modifiers, ToolInput};

use crate::kit::style::Tip;
use crate::window::ribbon::Corners;

/// The width of the entries' kind column (« Début de glisser » fits).
const KIND_WIDTH: f32 = 120.0;
/// The pilot's mark before an entry it caused.
const MARK: f32 = 14.0;

/// The window's state, while it is open.
#[derive(Clone, Debug)]
pub struct JournalWindow {
    tab: Tab,
    /// The kinds of entries shown.
    shown: Shown,
}

impl Default for JournalWindow {
    fn default() -> Self {
        JournalWindow {
            tab: Tab::Log,
            shown: Shown { inputs: true, hovers: false, tools: true, operations: true, jobs: true, origin: Who::All },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Log,
    Failures,
}

#[derive(Clone, Copy, Debug)]
struct Shown {
    inputs: bool,
    /// The hovers among the inputs: the most of them, so left out at first.
    hovers: bool,
    /// Tools and their options.
    tools: bool,
    /// Operations applied or failed, undo, redo.
    operations: bool,
    /// Kernel jobs back.
    jobs: bool,
    /// Whose entries.
    origin: Who,
}

/// Whose entries are shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Who {
    All,
    User,
    Pilot,
}

impl Who {
    fn label(self) -> String {
        match self {
            Who::All => t!("journal.origin.all"),
            Who::User => t!("journal.origin.user"),
            Who::Pilot => t!("journal.origin.pilot"),
        }
    }
}

impl Shown {
    fn shows(&self, entry: &Entry) -> bool {
        let whose = match self.origin {
            Who::All => true,
            Who::User => !entry.origin.is_control(),
            Who::Pilot => entry.origin.is_control(),
        };
        whose && self.shows_kind(&entry.event)
    }

    fn shows_kind(&self, e: &Event) -> bool {
        match e {
            Event::Input { input: ToolInput::Hover { .. }, .. } => self.inputs && self.hovers,
            Event::Input { .. } => self.inputs,
            Event::Tool(_) | Event::Option { .. } => self.tools,
            Event::Applied { .. } | Event::Welded { .. } | Event::Failed { .. } | Event::Undo | Event::Redo | Event::Forgotten { .. } => {
                self.operations
            }
            Event::Job { .. } => self.jobs,
        }
    }
}

/// The window's id, fixed: the title changes with the document and the language (its place
/// kept at launch, `SkraftApp::new`).
pub fn window_id() -> egui::Id {
    egui::Id::unique("journal_window")
}

/// Shows the window, on the journal of the shown document (`None`: the start page); closed,
/// `state` becomes `None`.
pub fn show(ctx: &egui::Context, state: &mut Option<JournalWindow>, doc: Option<(String, &Journal)>) {
    let Some(w) = state else { return };
    let mut open = true;
    let title = match &doc {
        Some((name, _)) => t!("journal.title_of", name = name),
        None => t!("journal.title"),
    };
    crate::kit::style::app_window(ctx, title, window_id(), &mut open, 620.0, None)
        .resizable(true)
        .default_height(420.0)
        .min_width(crate::kit::style::window_width(ctx, 560.0))
        .min_height(240.0)
        .show(ctx, |ui| {
            let Some((_, journal)) = doc else {
                ui.weak(t!("journal.no_document"));
                return;
            };
            let failures = journal.failures().count();
            // The ribbon's tabs, their band across the window under its title bar.
            let labels = [t!("journal.tab.log"), tp!("journal.tab.failures", failures)];
            let ids = [egui::Id::unique("journal_tab_log"), egui::Id::unique("journal_tab_failures")];
            let shown = match w.tab {
                Tab::Log => 0,
                Tab::Failures => 1,
            };
            // Up to the title bar's line: the window's margin and a spacing above the content.
            let m = ui.style().spacing.window_margin;
            let inset = egui::Margin { top: m.top + ui.spacing().item_spacing.y as i8, ..m };
            match crate::window::ribbon::tab_strip(ui, &labels, &ids, Some(shown), ui.available_width(), inset, Corners::Square, &[]) {
                Some(0) => w.tab = Tab::Log,
                Some(_) => w.tab = Tab::Failures,
                None => {}
            }
            if let Some(r) = journal.recording() {
                ui.colored_label(ui.visuals().error_fg_color, tp!("journal.recording", r.steps.len()));
            }
            // Each tab ends with a scroll area taking the rest, even empty: the window keeps
            // its size from one to the other. Nothing else may take the whole height (a
            // right-to-left layout does): the window would grow with it at each frame.
            match w.tab {
                Tab::Log => log(ui, &mut w.shown, journal),
                Tab::Failures => failures_list(ui, journal),
            }
        });
    if !open {
        *state = None;
    }
}

/// The journal's entries, oldest first, following the last.
fn log(ui: &mut egui::Ui, shown: &mut Shown, journal: &Journal) {
    let entries: Vec<&Entry> = journal.since(0).filter(|e| shown.shows(e)).collect();
    egui::Sides::new().show(
        ui,
        |ui| {
            ui.checkbox(&mut shown.inputs, t!("journal.show.inputs"));
            ui.add_enabled(shown.inputs, egui::Checkbox::new(&mut shown.hovers, t!("journal.show.hovers")));
            ui.checkbox(&mut shown.tools, t!("journal.show.tools"));
            ui.checkbox(&mut shown.operations, t!("journal.show.operations"));
            ui.checkbox(&mut shown.jobs, t!("journal.show.jobs"));
            ui.separator();
            egui::ComboBox::from_id_salt("journal_origin").selected_text(shown.origin.label()).show_ui(ui, |ui| {
                for who in [Who::All, Who::User, Who::Pilot] {
                    ui.selectable_value(&mut shown.origin, who, who.label());
                }
            });
        },
        |ui| {
            if ui.button(t!("journal.copy")).tip(t!("journal.copy_tip")).clicked() {
                let lines: Vec<String> = entries.iter().map(|e| skraft_control::json::entry(e).to_string()).collect();
                ui.ctx().copy_text(lines.join("\n"));
            }
        },
    );
    ui.separator();
    if entries.is_empty() {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| ui.weak(t!("journal.empty")));
        return;
    }
    let height = ui.text_style_height(&egui::TextStyle::Monospace) + 2.0;
    let weak = ui.visuals().weak_text_color();
    let alert = ui.visuals().error_fg_color;
    egui::ScrollArea::both().auto_shrink([false, false]).stick_to_bottom(true).show_rows(ui, height, entries.len(), |ui, range| {
        for e in &entries[range] {
            let (kind, detail, failed) = describe(&e.event);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.label(egui::RichText::new(format!("{:>5}", e.seq)).monospace().color(weak));
                ui.label(egui::RichText::new(clock(e.ms)).monospace().color(weak));
                pilot_mark(ui, &e.origin, height);
                // A column of its own: the details line up.
                let kind = egui::RichText::new(kind).strong();
                let kind = if failed { kind.color(alert) } else { kind };
                ui.allocate_ui_with_layout(egui::vec2(KIND_WIDTH, height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.set_min_width(KIND_WIDTH);
                    ui.add(egui::Label::new(kind).truncate());
                });
                let detail = egui::RichText::new(detail).monospace();
                ui.add(egui::Label::new(if failed { detail.color(alert) } else { detail }).extend());
            });
        }
    });
}

/// The pilot's mark (the control channel's, in its green) before what a client caused, its
/// name on hover; the user's entries: the room alone, the columns lined up.
fn pilot_mark(ui: &mut egui::Ui, origin: &Origin, height: f32) {
    let (rect, r) = ui.allocate_exact_size(egui::vec2(MARK, height), egui::Sense::hover());
    if !origin.is_control() {
        return;
    }
    let img = egui::Image::new(crate::kit::icons::REMOTE).tint(crate::window::titlebar::ACTIVE);
    img.paint_at(ui, egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(MARK)));
    r.tip(match origin.client() {
        Some(c) => t!("journal.origin.pilot_named", client = c),
        None => t!("journal.origin.pilot_tip"),
    });
}

/// The kernel's failures, the last first, each with its job to copy (run again headless by
/// `skraft_kernel::run`).
fn failures_list(ui: &mut egui::Ui, journal: &Journal) {
    let failures: Vec<&Failure> = journal.failures().collect();
    let weak = ui.visuals().weak_text_color();
    let alert = ui.visuals().error_fg_color;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        if failures.is_empty() {
            ui.weak(t!("journal.no_failures"));
        }
        for f in failures.iter().rev() {
            egui::Sides::new().show(
                ui,
                |ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    ui.label(egui::RichText::new(format!("{:>5}", f.seq)).monospace().color(weak));
                    ui.label(egui::RichText::new(clock(f.ms)).monospace().color(weak));
                    let what = f.op.as_ref().map_or_else(|| t!("journal.job.mesh"), |o| o.label());
                    ui.label(egui::RichText::new(what).strong());
                },
                |ui| {
                    if ui.button(t!("journal.copy_job")).tip(t!("journal.copy_job_tip")).clicked() {
                        let full = skraft_control::json::failure(f, true);
                        ui.ctx().copy_text(serde_json::to_string_pretty(&full).unwrap_or_default());
                    }
                },
            );
            ui.label(egui::RichText::new(&f.error).monospace().color(alert));
            ui.weak(t!("journal.job_size", inputs = f.job.inputs.len(), tasks = f.job.tasks.len()));
            ui.separator();
        }
    });
}

/// An event's kind, its detail, and whether it tells a failure.
fn describe(e: &Event) -> (String, String, bool) {
    match e {
        Event::Input { input, count } => {
            let (kind, detail) = input_text(input);
            let detail = if *count > 1 { format!("{detail}  ×{count}") } else { detail };
            (kind, detail, false)
        }
        Event::Tool(k) => (t!("journal.event.tool"), k.label(), false),
        Event::Option { key, value } => {
            let value = serde_json::to_string(value).unwrap_or_else(|_| format!("{value:?}"));
            (t!("journal.event.option"), format!("{key} = {value}"), false)
        }
        Event::Applied { op, revision } => (t!("journal.event.applied"), t!("journal.applied", op = op, revision = revision), false),
        Event::Welded { part, body, reference, distance } => {
            let detail = t!("journal.welded", part = part, body = body, reference = reference, distance = format!("{distance:.6}"));
            (t!("journal.event.welded"), detail, false)
        }
        Event::Failed { op, error } => (t!("journal.event.failed"), format!("{op} — {error}"), true),
        Event::Job { what, ms, error } => {
            let what = match what {
                JobKind::Operation => t!("journal.job.operation"),
                JobKind::Mesh => t!("journal.job.mesh"),
            };
            let detail = match error {
                Some(err) => format!("{what} · {ms:.1} ms — {err}"),
                None => format!("{what} · {ms:.1} ms"),
            };
            (t!("journal.event.job"), detail, error.is_some())
        }
        Event::Undo => (t!("journal.event.undo"), String::new(), false),
        Event::Redo => (t!("journal.event.redo"), String::new(), false),
        Event::Forgotten { steps, bytes } => (
            t!("journal.event.forgotten"),
            tp!("journal.forgotten", *steps as usize, steps = steps, bytes = crate::workspaces::common::viewport::bytes(*bytes)),
            false,
        ),
    }
}

/// An input's kind and detail (its pixel, its modifiers, its text).
fn input_text(i: &ToolInput) -> (String, String) {
    let at = |px: &glam::DVec2| format!("({:.0}, {:.0})", px.x, px.y);
    let with = |px: &glam::DVec2, m: &Modifiers| format!("{}{}", at(px), mods(m));
    match i {
        ToolInput::Hover { px } => (t!("journal.input.hover"), at(px)),
        ToolInput::Click { px, mods: m } => (t!("journal.input.click"), with(px, m)),
        ToolInput::DoubleClick { px, mods: m } => (t!("journal.input.double_click"), with(px, m)),
        ToolInput::DragStart { px, mods: m } => (t!("journal.input.drag_start"), with(px, m)),
        ToolInput::Drag { px } => (t!("journal.input.drag"), at(px)),
        ToolInput::DragEnd { px, mods: m } => (t!("journal.input.drag_end"), with(px, m)),
        ToolInput::Key { key, mods: m } => (t!("journal.input.key"), format!("{key:?}{}", mods(m))),
        ToolInput::Vcb(text) => (t!("journal.input.vcb"), format!("« {text} »")),
        ToolInput::Value(v) => (t!("journal.input.value"), format!("{v:?}")),
    }
}

/// The modifiers held, after an input's detail.
fn mods(m: &Modifiers) -> String {
    let mut s = String::new();
    if m.toggle {
        s += &format!(" + {}", t!("journal.mod.toggle"));
    }
    if m.copy {
        s += &format!(" + {}", t!("journal.mod.copy"));
    }
    s
}

/// A time since the editor started (ms), as `m:ss.mmm`.
fn clock(ms: f64) -> String {
    let ms = ms.max(0.0) as u64;
    format!("{}:{:02}.{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
}

#[cfg(test)]
mod tests {
    use super::clock;

    #[test]
    fn clock_reads_minutes_seconds_millis() {
        assert_eq!(clock(0.0), "0:00.000");
        assert_eq!(clock(61_234.9), "1:01.234");
        assert_eq!(clock(3_600_000.0), "60:00.000");
    }
}

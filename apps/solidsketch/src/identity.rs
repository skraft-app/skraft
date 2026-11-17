//! What this app is, for whoever has several running (docs/ARCHITECTURE.md §7 septies): its
//! build — the git worktree, branch and commit the build script found, dev builds only — and,
//! on the desktop, its file in the registry of running apps
//! (`solidsketch_control::instances`), which a client of the control channel reads to find the
//! app it means. The title bar shows it as a badge, coloured after the worktree — or after
//! the Claude Code session that launched it, when that session is named after a colour.

use solidsketch_control::instances::Build;

/// What this build was made from: dev builds only (none in a release, nor without git).
pub fn build() -> Option<Build> {
    #[cfg(debug_assertions)]
    {
        let worktree = option_env!("SOLIDSKETCH_GIT_WORKTREE")?;
        Some(Build {
            worktree: worktree.to_owned(),
            branch: option_env!("SOLIDSKETCH_GIT_BRANCH").unwrap_or_default().to_owned(),
            commit: option_env!("SOLIDSKETCH_GIT_COMMIT").unwrap_or_default().to_owned(),
            dirty: option_env!("SOLIDSKETCH_GIT_DIRTY") == Some("1"),
            built: option_env!("SOLIDSKETCH_BUILT").and_then(|s| s.parse().ok()).unwrap_or(0),
        })
    }
    #[cfg(not(debug_assertions))]
    None
}

/// The colours a Claude Code session may be named after (`/color`'s, in English and in
/// French), and their hues in degrees.
const NAMED: [(&[&str], f32); 8] = [
    (&["red", "rouge"], 0.0),
    (&["orange"], 30.0),
    (&["yellow", "jaune"], 55.0),
    (&["green", "vert"], 125.0),
    (&["cyan"], 185.0),
    (&["blue", "bleu"], 215.0),
    (&["purple", "violet"], 275.0),
    (&["pink", "rose"], 330.0),
];

/// The badge's colour for the build `label`, launched by the Claude Code session named
/// `session`: the colour that name says, if one of [`NAMED`]; else a hue of the label's own (a
/// hash of it), the same at every launch. Light enough under dark text.
pub fn color(label: &str, session: Option<&str>) -> egui::Color32 {
    if let Some(hue) = session.and_then(named_hue) {
        return egui::ecolor::Hsva::new(hue, 0.55, 0.88, 1.0).into();
    }
    // FNV-1a: stable across runs and builds (std's hasher is not).
    let hash = label.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    let hue = (hash % 360) as f32 / 360.0;
    egui::ecolor::Hsva::new(hue, 0.45, 0.85, 1.0).into()
}

/// The hue (0..1) of the colour `name` is, if one of [`NAMED`] (any case, spaces around).
fn named_hue(name: &str) -> Option<f32> {
    let name = name.trim().to_lowercase();
    NAMED.iter().find(|(names, _)| names.contains(&name.as_str())).map(|(_, hue)| hue / 360.0)
}

/// The name of the Claude Code session that launched this dev build, as it is now: Claude
/// Code keeps each running session in `~/.claude/sessions/<its pid>.json` (`sessionId`,
/// `name`), which a thread reads every [`Session::POLL`] — a `/rename` shows at once.
#[cfg(not(target_arch = "wasm32"))]
pub struct Session {
    name: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Session {
    const POLL: std::time::Duration = std::time::Duration::from_secs(2);

    /// Its watch, repainting `ctx` when the name changes; none in a release or when not
    /// launched by Claude Code (`CLAUDE_CODE_SESSION_ID`, `CLAUDE_PID`).
    pub fn watch(ctx: egui::Context) -> Option<Self> {
        build()?;
        let id = std::env::var("CLAUDE_CODE_SESSION_ID").ok().filter(|s| !s.is_empty())?;
        let dir = dirs::home_dir()?.join(".claude").join("sessions");
        let pid = std::env::var("CLAUDE_PID").ok();
        let name = std::sync::Arc::new(std::sync::Mutex::new(None));
        let shared = std::sync::Arc::downgrade(&name);
        let spawned = std::thread::Builder::new().name("claude-session".into()).spawn(move || {
            let mut file = None;
            // Until the app is gone.
            while let Some(name) = shared.upgrade() {
                let now = match file.as_deref().and_then(|f| session_name(f, &id)) {
                    Some(now) => now,
                    None => {
                        file = session_file(&dir, pid.as_deref(), &id);
                        file.as_deref().and_then(|f| session_name(f, &id)).flatten()
                    }
                };
                {
                    let mut shown = name.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    if *shown != now {
                        *shown = now;
                        ctx.request_repaint();
                    }
                }
                drop(name);
                std::thread::sleep(Self::POLL);
            }
        });
        spawned.inspect_err(|e| log::warn!("Claude Code session not watched: {e}")).ok()?;
        Some(Self { name })
    }

    /// Its name, if it has one.
    pub fn name(&self) -> Option<String> {
        self.name.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }
}

/// The file of the session `id` in Claude Code's sessions folder `dir`: its process's
/// (`pid`) if that one says so, else the one that does.
#[cfg(not(target_arch = "wasm32"))]
fn session_file(dir: &std::path::Path, pid: Option<&str>, id: &str) -> Option<std::path::PathBuf> {
    let own = pid.map(|p| dir.join(format!("{p}.json"))).filter(|f| session_name(f, id).is_some());
    own.or_else(|| {
        let entries = std::fs::read_dir(dir).ok()?;
        let files = entries.filter_map(|e| e.ok().map(|e| e.path()));
        files.filter(|f| f.extension().is_some_and(|x| x == "json")).find(|f| session_name(f, id).is_some())
    })
}

/// The name the session file `file` gives, if it is the session `id`'s (none: not, or
/// unreadable; `Some(None)`: no name).
#[cfg(not(target_arch = "wasm32"))]
fn session_name(file: &std::path::Path, id: &str) -> Option<Option<String>> {
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
    (json.get("sessionId")?.as_str()? == id)
        .then(|| json.get("name").and_then(|n| n.as_str()).map(str::trim).filter(|n| !n.is_empty()).map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_keeps_its_colour_and_two_differ() {
        assert_eq!(color("prism-z", None), color("prism-z", None));
        assert_ne!(color("prism-z", None), color("instance-identity", None));
    }

    #[test]
    fn a_session_named_after_a_colour_colours_the_badge() {
        assert_eq!(color("prism-z", Some("Red")), color("axis", Some(" rouge ")));
        assert_ne!(color("prism-z", Some("red")), color("prism-z", Some("blue")));
        // Any other name: the label's own.
        assert_eq!(color("prism-z", Some("AXIS")), color("prism-z", None));
        assert_eq!(color("prism-z", Some("dark red")), color("prism-z", None));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_session_file_gives_its_name() {
        let dir = std::env::temp_dir().join(format!("solidsketch-sessions-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("1.json"), r#"{"pid":1,"sessionId":"a","name":" blue "}"#).unwrap();
        std::fs::write(dir.join("2.json"), r#"{"pid":2,"sessionId":"b"}"#).unwrap();
        let a = session_file(&dir, Some("9"), "a").unwrap();
        assert_eq!(session_name(&a, "a"), Some(Some("blue".to_owned())));
        assert_eq!(session_name(&a, "b"), None);
        let b = session_file(&dir, Some("2"), "b").unwrap();
        assert_eq!(session_name(&b, "b"), Some(None));
        assert_eq!(session_file(&dir, None, "c"), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_dev_build_knows_its_worktree() {
        // The tests are a dev build, from a checkout.
        if let Some(b) = build() {
            assert!(!b.commit.is_empty() && std::path::Path::new(&b.worktree).join(".git").exists(), "{b:?}");
        }
    }
}

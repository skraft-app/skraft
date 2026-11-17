//! The registry of the SolidSketch apps running on this computer (docs/ARCHITECTURE.md
//! §7 septies): several may run at once — one per worktree, launched by the user or by an
//! agent's session — and a client must find the one it means, not merely the last started.
//!
//! Each app writes `<dir>/instances/<pid>.json` (an [`Instance`]: its pid, its control
//! channel's port while on, its build, who launched it) and holds a lock on `<pid>.lock`
//! while it runs; both go when it quits. The OS lets the lock go with a process that dies, so
//! [`list`] tells a live app from a crashed one's leftovers (and removes those).

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// What a build was made from (dev builds only: `solidsketch`'s build script).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Build {
    /// The checkout's folder (a git worktree).
    pub worktree: String,
    pub branch: String,
    /// The commit, short.
    pub commit: String,
    /// Changes not committed went into it.
    pub dirty: bool,
    /// When it was built (seconds since the Unix epoch).
    pub built: u64,
}

impl Build {
    /// Its short name: the worktree's (`solidsketch-prism-z` is `prism-z`), the branch's for
    /// the main checkout.
    pub fn label(&self) -> String {
        worktree_label(Path::new(&self.worktree)).unwrap_or_else(|| self.branch.clone())
    }
}

/// The short name of the worktree folder `path`: its name without `solidsketch-`; none for the
/// main checkout (`solidsketch`).
pub fn worktree_label(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    name.strip_prefix("solidsketch-").filter(|s| !s.is_empty()).map(str::to_owned)
}

/// A running app.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instance {
    pub pid: u32,
    /// When it started (seconds since the Unix epoch).
    pub started: u64,
    pub version: String,
    /// Its executable.
    pub exe: Option<String>,
    /// Its control channel's, while on.
    pub port: Option<u16>,
    /// What it was built from (dev builds).
    pub build: Option<Build>,
    /// The Claude Code session that launched it (`CLAUDE_CODE_SESSION_ID`).
    pub claude_session: Option<String>,
    /// Who launched it, as they said (`SOLIDSKETCH_LAUNCHER`).
    pub launcher: Option<String>,
}

impl Instance {
    /// This process.
    pub fn here(version: &str, build: Option<Build>) -> Self {
        let env = |k| std::env::var(k).ok().map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
        Instance {
            pid: std::process::id(),
            started: now(),
            version: version.to_owned(),
            exe: std::env::current_exe().ok().map(|p| p.display().to_string()),
            port: None,
            build,
            claude_session: env("CLAUDE_CODE_SESSION_ID"),
            launcher: env("SOLIDSKETCH_LAUNCHER"),
        }
    }

    /// Its short name: its build's, else its version's.
    pub fn label(&self) -> String {
        self.build.as_ref().map_or_else(|| self.version.clone(), Build::label)
    }
}

/// Seconds since the Unix epoch.
pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The registry's folder in the control folder `dir` (`crate::server::default_dir`).
pub fn registry_dir(dir: &Path) -> PathBuf {
    dir.join("instances")
}

/// This app in the registry, while it lives: dropping it takes it out.
pub struct Registration {
    dir: PathBuf,
    info: Instance,
    /// Held while the app runs.
    lock: Option<File>,
}

impl Registration {
    /// Writes `info` to the registry in `dir` (the control folder) and takes its lock.
    pub fn register(dir: &Path, info: Instance) -> std::io::Result<Self> {
        let dir = registry_dir(dir);
        std::fs::create_dir_all(&dir)?;
        let lock = File::create(dir.join(format!("{}.lock", info.pid)))?;
        lock.try_lock().map_err(std::io::Error::other)?;
        let r = Registration { dir, info, lock: Some(lock) };
        r.write()?;
        Ok(r)
    }

    pub fn info(&self) -> &Instance {
        &self.info
    }

    /// Its control channel's port now (none: off).
    pub fn set_port(&mut self, port: Option<u16>) -> std::io::Result<()> {
        if self.info.port == port {
            return Ok(());
        }
        self.info.port = port;
        self.write()
    }

    /// Written whole, then renamed: a reader never sees half of it.
    fn write(&self) -> std::io::Result<()> {
        let path = self.dir.join(format!("{}.json", self.info.pid));
        let part = self.dir.join(format!("{}.json.part", self.info.pid));
        let mut f = File::create(&part)?;
        f.write_all(serde_json::to_string_pretty(&self.info).map_err(std::io::Error::other)?.as_bytes())?;
        f.write_all(b"\n")?;
        drop(f);
        std::fs::rename(part, path)
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.dir.join(format!("{}.json", self.info.pid)));
        // Closed before removed (Windows).
        drop(self.lock.take());
        let _ = std::fs::remove_file(self.dir.join(format!("{}.lock", self.info.pid)));
    }
}

/// The apps running, the last started first; the files of those gone (their lock free) are
/// removed.
pub fn list(dir: &Path) -> Vec<Instance> {
    let dir = registry_dir(dir);
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let Some(pid) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        let lock = dir.join(format!("{pid}.lock"));
        let alive = File::open(&lock).is_ok_and(|f| matches!(f.try_lock(), Err(std::fs::TryLockError::WouldBlock)));
        if !alive {
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_file(&lock);
            continue;
        }
        if let Some(i) = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Instance>(&t).ok()) {
            out.push(i);
        }
    }
    out.sort_by(|a, b| b.started.cmp(&a.started).then(b.pid.cmp(&a.pid)));
    out
}

/// The app a client means, among those whose channel is on (`list`'s order: the last
/// started first): the one its Claude Code `session` launched, else one built from the
/// worktree `cwd` is in. None: neither (the client falls back on the discovery file).
pub fn choose<'a>(instances: &'a [Instance], session: Option<&str>, cwd: Option<&Path>) -> Option<&'a Instance> {
    let on = || instances.iter().filter(|i| i.port.is_some());
    if let Some(s) = session
        && let Some(i) = on().find(|i| i.claude_session.as_deref() == Some(s))
    {
        return Some(i);
    }
    let cwd = cwd?;
    on().find(|i| i.build.as_ref().is_some_and(|b| !b.worktree.is_empty() && cwd.starts_with(&b.worktree)))
}

/// The registry as `app.instances` answers it.
pub fn list_json(dir: &Path) -> serde_json::Value {
    serde_json::to_value(list(dir)).unwrap_or_default()
}

/// A client's name for the `X-SolidSketch-Client` header: `tool`, the Claude Code session it
/// runs in (its id's first 8 characters), the worktree it runs from.
pub fn client_name(tool: &str) -> String {
    let mut name = tool.to_owned();
    if let Ok(s) = std::env::var("CLAUDE_CODE_SESSION_ID")
        && !s.trim().is_empty()
    {
        name.push_str(&format!(" · session {}", s.trim().chars().take(8).collect::<String>()));
    }
    if let Some(label) = std::env::current_dir().ok().and_then(|d| checkout(&d)).and_then(|w| worktree_label(&w)) {
        name.push_str(&format!(" · {label}"));
    }
    name
}

/// The checkout `path` is in: the nearest folder up holding `.git`.
pub fn checkout(path: &Path) -> Option<PathBuf> {
    path.ancestors().find(|p| p.join(".git").exists()).map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("solidsketch-instances-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn instance(pid: u32, started: u64, port: Option<u16>, session: Option<&str>, worktree: &str) -> Instance {
        Instance {
            pid,
            started,
            version: "0.1.0".into(),
            exe: None,
            port,
            build: Some(Build { worktree: worktree.into(), branch: "main".into(), commit: "abc1234".into(), dirty: false, built: 0 }),
            claude_session: session.map(str::to_owned),
            launcher: None,
        }
    }

    #[test]
    fn an_app_is_listed_while_it_lives_and_leftovers_go() {
        let dir = temp_dir("list");
        let mut r = Registration::register(&dir, instance(4_000_001, 10, None, None, "/w/solidsketch")).unwrap();
        r.set_port(Some(7868)).unwrap();
        assert_eq!(list(&dir).iter().map(|i| (i.pid, i.port)).collect::<Vec<_>>(), [(4_000_001, Some(7868))]);
        // A crashed app's files: no lock held.
        let reg = registry_dir(&dir);
        std::fs::write(reg.join("4000002.json"), serde_json::to_string(&instance(4_000_002, 20, Some(1), None, "")).unwrap()).unwrap();
        std::fs::write(reg.join("4000002.lock"), "").unwrap();
        assert_eq!(list(&dir).len(), 1);
        assert!(!reg.join("4000002.json").exists() && !reg.join("4000002.lock").exists());
        drop(r);
        assert!(list(&dir).is_empty());
        assert_eq!(std::fs::read_dir(&reg).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_client_finds_its_session_s_app_then_its_worktree_s() {
        let all = vec![
            instance(3, 30, Some(7003), None, "/w/solidsketch"),
            instance(2, 20, Some(7002), Some("s-1"), "/w/solidsketch-prism-z"),
            instance(1, 10, None, Some("s-2"), "/w/solidsketch-x"),
        ];
        let port = |s, cwd: &str| choose(&all, s, Some(Path::new(cwd))).map(|i| i.pid);
        assert_eq!(port(Some("s-1"), "/w/solidsketch"), Some(2));
        // Its session's app has its channel off: the worktree's.
        assert_eq!(port(Some("s-2"), "/w/solidsketch-prism-z/crates"), Some(2));
        // `solidsketch` is not a parent of `solidsketch-x` (by components).
        assert_eq!(port(None, "/w/solidsketch-x"), None);
        assert_eq!(port(None, "/w/solidsketch/apps"), Some(3));
    }

    #[test]
    fn labels() {
        assert_eq!(worktree_label(Path::new("/w/solidsketch-prism-z")).as_deref(), Some("prism-z"));
        assert_eq!(worktree_label(Path::new("/w/solidsketch")), None);
        assert_eq!(instance(1, 0, None, None, "/w/solidsketch").label(), "main");
        assert_eq!(instance(1, 0, None, None, "/w/solidsketch-prism-z").label(), "prism-z");
    }
}

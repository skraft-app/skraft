//! The check for a new version (docs/ui/updates.md, decision 0053), on the desktop: reads the
//! latest release's manifest on a thread, says so in the title bar and a toast, and opens this
//! platform's installer in the browser. It never installs anything itself.

use std::collections::BTreeMap;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use skraft_i18n::t;

use crate::window::toasts::{Act, Note};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const REPOSITORY: &str = "https://github.com/skraft-app/skraft";
/// A static file of the latest release: no API, so no rate limit per address.
const MANIFEST: &str = "https://github.com/skraft-app/skraft/releases/latest/download/latest.json";
/// At most one automatic check per this many seconds.
const PERIOD: u64 = 24 * 60 * 60;
/// An automatic check waits this long after the launch, which it must not slow down.
const LAUNCH_DELAY: Duration = Duration::from_secs(5);
const TIMEOUT: Duration = Duration::from_secs(15);
/// The most a manifest may weigh, in bytes.
const MAX_MANIFEST: u64 = 256 * 1024;

/// A release, as its `latest.json` says (`scripts/update-manifest.sh`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    /// Its CHANGELOG.md entry.
    #[serde(default)]
    pub notes: String,
    /// The installer's URL, by platform (`macos-arm64`…).
    #[serde(default)]
    pub assets: BTreeMap<String, String>,
}

/// What the app keeps about the checks, in the preferences.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// Check by itself, at launch.
    pub check: bool,
    /// The last check that answered, in seconds since the epoch.
    pub last_check: u64,
    /// What it found: still shown at the launches that do not check.
    pub latest: Option<Release>,
    /// The version the user chose to ignore.
    pub skipped: Option<String>,
    /// The last version a toast announced: once each.
    pub announced: Option<String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { check: true, last_check: 0, latest: None, skipped: None, announced: None }
    }
}

impl Prefs {
    /// The release newer than this build, if one is known.
    fn newer(&self) -> Option<&Release> {
        self.latest.as_ref().filter(|r| is_newer(&r.version, VERSION))
    }

    /// The version the title bar announces: a newer one, not ignored.
    pub fn announce(&self) -> Option<&str> {
        let r = self.newer().filter(|_| self.check)?;
        (self.skipped.as_deref() != Some(&r.version)).then_some(r.version.as_str())
    }
}

/// `major.minor.patch`, nothing else.
fn parse(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.').map(|p| if p.bytes().all(|b| b.is_ascii_digit()) { p.parse().ok() } else { None });
    let v = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(v)
}

fn is_newer(version: &str, than: &str) -> bool {
    matches!((parse(version), parse(than)), (Some(a), Some(b)) if a > b)
}

/// This build's key among a manifest's assets.
fn platform() -> String {
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        arch => arch,
    };
    format!("{}-{arch}", std::env::consts::OS)
}

/// Where to get `release` on `platform`: its installer, only from this repository's files of
/// that very version; else the release's page.
fn download_url(release: &Release, platform: &str) -> String {
    let files = format!("{REPOSITORY}/releases/download/v{}/", release.version);
    match release.assets.get(platform) {
        Some(url) if url.strip_prefix(&files).is_some_and(|f| !f.is_empty() && !f.contains(['/', '?', '#', '\\'])) => url.clone(),
        _ => format!("{REPOSITORY}/releases/tag/v{}", release.version),
    }
}

fn read_manifest(text: &str) -> Result<Release, String> {
    let release: Release = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if parse(&release.version).is_none() {
        return Err(format!("not a version: {}", release.version));
    }
    Ok(release)
}

/// A debug build may read another manifest, to try this; a release build never does.
fn manifest_override() -> Option<String> {
    if cfg!(debug_assertions) { std::env::var("SKRAFT_UPDATE_URL").ok() } else { None }
}

fn fetch(url: &str) -> Result<Release, String> {
    use ureq::tls::{Certificate, RootCerts, TlsConfig};
    // The system's certificates: those a proxy of the user's network may add are among them.
    let roots: Vec<_> =
        rustls_native_certs::load_native_certs().certs.iter().map(|c| Certificate::from_der(c.as_ref()).to_owned()).collect();
    let tls = TlsConfig::builder()
        .root_certs(RootCerts::new_with_certs(&roots))
        .unversioned_rustls_crypto_provider(std::sync::Arc::new(rustls::crypto::ring::default_provider()))
        .build();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .user_agent(concat!("Skraft/", env!("CARGO_PKG_VERSION")))
        .tls_config(tls)
        .build()
        .into();
    let mut response = agent.get(url).call().map_err(|e| e.to_string())?;
    let text = response.body_mut().with_config().limit(MAX_MANIFEST).read_to_string().map_err(|e| e.to_string())?;
    read_manifest(&text)
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// An automatic check is due: none answered for a period (or the clock went back).
fn due(last_check: u64, now: u64) -> bool {
    now < last_check || now - last_check >= PERIOD
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Status {
    Idle,
    Checking,
    UpToDate,
    Failed(String),
}

/// The window's id.
const WINDOW_ID: &str = "update_window";

/// The checks of this run, and their window.
pub struct Updates {
    ctx: egui::Context,
    pending: Option<Receiver<Result<Release, String>>>,
    /// The user asked: the window tells how it went.
    status: Status,
    window: bool,
}

impl Updates {
    pub fn new(ctx: &egui::Context) -> Updates {
        Updates { ctx: ctx.clone(), pending: None, status: Status::Idle, window: false }
    }

    /// At launch: checks if it is time. Debug builds do not, unless told where to look.
    pub fn start(&mut self, prefs: &Prefs) {
        let url = manifest_override();
        if prefs.check && (url.is_some() || (!cfg!(debug_assertions) && due(prefs.last_check, now()))) {
            self.spawn(url.unwrap_or_else(|| MANIFEST.to_owned()), LAUNCH_DELAY);
        }
    }

    /// The user asks: the window, and a check unless a newer version is already known.
    pub fn open(&mut self, prefs: &Prefs) {
        self.window = true;
        // Over the window it was asked from (« À propos »).
        self.ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, egui::Id::unique(WINDOW_ID)));
        if prefs.newer().is_some() {
            return;
        }
        self.status = Status::Checking;
        if self.pending.is_none() {
            self.spawn(manifest_override().unwrap_or_else(|| MANIFEST.to_owned()), Duration::ZERO);
        }
    }

    fn spawn(&mut self, url: String, delay: Duration) {
        let (tx, rx) = channel();
        let ctx = self.ctx.clone();
        let thread = std::thread::Builder::new().name("update-check".to_owned()).spawn(move || {
            std::thread::sleep(delay);
            let _ = tx.send(fetch(&url));
            ctx.request_repaint();
        });
        match thread {
            Ok(_) => self.pending = Some(rx),
            Err(e) => log::warn!("the update check: {e}"),
        }
    }

    /// Takes a check's answer; a toast for a version not announced yet.
    pub fn poll(&mut self, prefs: &mut Prefs) -> Option<Note> {
        let answer = match self.pending.as_ref()?.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err("the check stopped".to_owned()),
        };
        self.pending = None;
        match answer {
            Ok(release) => {
                prefs.last_check = now();
                prefs.latest = Some(release);
                if self.status == Status::Checking {
                    self.status = if prefs.newer().is_some() { Status::Idle } else { Status::UpToDate };
                }
            }
            Err(e) => {
                // Silent unless asked for; the next launch tries again.
                log::info!("the update check: {e}");
                if self.status == Status::Checking {
                    self.status = Status::Failed(e);
                }
            }
        }
        let version = prefs.announce()?.to_owned();
        if self.window || prefs.announced.as_deref() == Some(&version) {
            return None;
        }
        prefs.announced = Some(version.clone());
        Some(Note::info(t!("update.toast", version = version)).with_action(t!("update.show"), Act::ShowUpdate))
    }

    /// The window, while open.
    pub fn show(&mut self, ctx: &egui::Context, prefs: &mut Prefs) {
        if !self.window {
            return;
        }
        let mut open = true;
        let mut close = false;
        egui::Window::new(t!("update.title"))
            .id(egui::Id::unique(WINDOW_ID))
            .title_frame(crate::kit::style::window_title_frame(ctx))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.set_width(420.0);
                match prefs.newer().cloned() {
                    Some(release) => close = available(ui, &release, prefs),
                    None => match &self.status {
                        Status::Failed(e) => {
                            ui.label(t!("update.failed"));
                            ui.small(e);
                        }
                        Status::UpToDate => {
                            ui.label(t!("update.up_to_date", version = VERSION));
                        }
                        Status::Idle | Status::Checking => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(t!("update.checking"));
                            });
                        }
                    },
                }
            });
        self.window = open && !close;
    }
}

/// A newer version: what it brings, and what to do. Whether the window is done.
fn available(ui: &mut egui::Ui, release: &Release, prefs: &mut Prefs) -> bool {
    ui.strong(t!("update.available", version = &release.version));
    ui.weak(t!("update.current", version = VERSION));
    ui.add_space(8.0);
    egui::ScrollArea::vertical().max_height(280.0).auto_shrink([false, true]).show(ui, |ui| notes(ui, &release.notes));
    ui.add_space(10.0);
    let mut done = false;
    ui.horizontal(|ui| {
        if ui.button(t!("update.download")).clicked() {
            crate::app::open_url(ui.ctx(), download_url(release, &platform()));
            done = true;
        }
        if ui.button(t!("update.later")).clicked() {
            done = true;
        }
        if ui.button(t!("update.skip")).clicked() {
            prefs.skipped = Some(release.version.clone());
            done = true;
        }
    });
    done
}

/// A changelog entry, its Markdown reduced to headings and bullets.
fn notes(ui: &mut egui::Ui, text: &str) {
    for line in text.lines() {
        let line = line.trim_end().replace("**", "");
        if let Some(heading) = line.strip_prefix("### ") {
            ui.add_space(4.0);
            ui.strong(heading);
        } else if let Some(item) = line.strip_prefix("- ") {
            ui.label(format!("• {item}"));
        } else if !line.is_empty() {
            ui.label(line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(version: &str, url: &str) -> Release {
        Release { version: version.to_owned(), notes: String::new(), assets: BTreeMap::from([("macos-arm64".to_owned(), url.to_owned())]) }
    }

    #[test]
    fn versions_compare_by_number() {
        assert!(is_newer("0.0.10", "0.0.9"));
        assert!(is_newer("0.1.0", "0.0.99"));
        assert!(!is_newer("0.0.2", "0.0.2"));
        assert!(!is_newer("0.0.1", "0.0.2"));
        for bad in ["", "1.2", "1.2.3.4", "1.2.x", "1.2.3-rc1", "+1.2.3", "1..3"] {
            assert!(!is_newer(bad, "0.0.1"), "{bad}");
        }
    }

    #[test]
    fn only_this_repository_s_files_of_that_version_are_opened() {
        let page = format!("{REPOSITORY}/releases/tag/v9.0.0");
        let good = format!("{REPOSITORY}/releases/download/v9.0.0/Skraft-9.0.0-macos-arm64.dmg");
        assert_eq!(download_url(&release("9.0.0", &good), "macos-arm64"), good);
        assert_eq!(download_url(&release("9.0.0", &good), "linux-x86_64"), page, "no file for this platform");
        for bad in [
            "https://example.com/Skraft.dmg".to_owned(),
            format!("{REPOSITORY}/releases/download/v8.0.0/Skraft.dmg"),
            format!("{REPOSITORY}/releases/download/v9.0.0/../../evil"),
            format!("{REPOSITORY}/releases/download/v9.0.0/"),
            format!("{REPOSITORY}.example.com/releases/download/v9.0.0/Skraft.dmg"),
        ] {
            assert_eq!(download_url(&release("9.0.0", &bad), "macos-arm64"), page, "{bad}");
        }
    }

    #[test]
    fn a_manifest_needs_a_version() {
        assert_eq!(read_manifest(r#"{"version":"1.2.3"}"#).unwrap().version, "1.2.3");
        assert!(read_manifest(r#"{"version":"latest"}"#).is_err());
        assert!(read_manifest("<html>").is_err());
    }

    #[test]
    fn a_check_is_due_once_a_day() {
        assert!(due(0, PERIOD));
        assert!(!due(PERIOD, PERIOD + 10));
        assert!(due(PERIOD, 2 * PERIOD));
        assert!(due(PERIOD, 10), "the clock went back");
    }

    #[test]
    fn the_bar_announces_a_newer_version_not_ignored() {
        let mut p = Prefs { latest: Some(release("99.0.0", "")), ..Prefs::default() };
        assert_eq!(p.announce(), Some("99.0.0"));
        p.skipped = Some("99.0.0".to_owned());
        assert_eq!(p.announce(), None);
        p.latest = Some(release("99.0.1", ""));
        assert_eq!(p.announce(), Some("99.0.1"), "the next one is announced");
        p.check = false;
        assert_eq!(p.announce(), None);
        p = Prefs { latest: Some(release(VERSION, "")), ..Prefs::default() };
        assert_eq!(p.announce(), None);
    }

    #[test]
    fn older_preferences_check_by_default() {
        let p: Prefs = serde_json::from_str("{}").unwrap();
        assert!(p.check);
    }
}

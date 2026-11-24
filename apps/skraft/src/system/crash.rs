//! What the app tells when it dies (desktop). Started from the Explorer, the Finder or a
//! launcher, it has no console (on Windows a release build is a GUI app): a panic or a window
//! that cannot open would make it vanish without a word. So the panic of the main thread — the
//! one that ends the app — and the error `eframe::run_native` returns are appended to
//! `crash.log` in the user's local data folder (`%LOCALAPPDATA%\Skraft` on Windows,
//! `~/Library/Application Support/Skraft` on macOS, `~/.local/share/Skraft` on Linux),
//! and shown in a dialog that names the file.
//!
//! The other threads' panics are left to the default hook: the kernel turns its own into errors
//! (`skraft_kernel::engine::guarded`), which the journal keeps.

use std::io::Write as _;
use std::path::PathBuf;

use skraft_i18n::t;

use crate::system::dates::civil_from_days;

/// Beyond, the log starts over: it keeps the last crashes, not years of them.
const MAX_LOG_BYTES: u64 = 1 << 20;

/// The file the crashes go to.
pub fn log_path() -> Option<PathBuf> {
    Some(dirs::data_local_dir()?.join("Skraft").join("crash.log"))
}

/// Installs the panic hook, first thing in `main`.
pub fn install() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default(info);
        if std::thread::current().name() != Some("main") {
            return;
        }
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".to_owned());
        let at = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_default();
        let backtrace = std::backtrace::Backtrace::force_capture();
        report(&format!("panic: {message}\nat {at}\n\nbacktrace:\n{backtrace}"), &message);
    }));
}

/// The app could not run: `error` is what `eframe::run_native` returned.
pub fn fatal(error: &dyn std::fmt::Display) {
    let error = error.to_string();
    log::error!("{error}");
    report(&format!("error: {error}"), &error);
}

/// Appends `details` to the log and shows `summary` in a dialog, with where the log is.
fn report(details: &str, summary: &str) {
    let logged =
        log_path().and_then(|path| append(&path, details).inspect_err(|e| eprintln!("{}: {e}", path.display())).ok().map(|()| path));
    let description = match &logged {
        Some(path) => t!("crash.message_logged", error = summary, path = path.display()),
        None => t!("crash.message", error = summary),
    };
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title(t!("crash.title"))
        .set_description(description)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

fn append(path: &std::path::Path, details: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let fresh = std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_LOG_BYTES);
    let mut file = std::fs::OpenOptions::new().create(true).write(true).append(!fresh).truncate(fresh).open(path)?;
    writeln!(
        file,
        "=== Skraft {} ({}-{}) — {}\n{details}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        utc_now()
    )
}

/// Now, `YYYY-MM-DD HH:MM:SS UTC`.
fn utc_now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    let s = secs % 86_400;
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC", s / 3600, s / 60 % 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_then_starts_over() {
        let dir = std::env::temp_dir().join(format!("skraft-crash-{}", std::process::id()));
        let path = dir.join("crash.log");
        let _ = std::fs::remove_dir_all(&dir);
        append(&path, "first").unwrap();
        append(&path, "second").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("first") && text.contains("second"));
        std::fs::write(&path, vec![b'x'; MAX_LOG_BYTES as usize + 1]).unwrap();
        append(&path, "third").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("=== Skraft") && text.contains("third"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

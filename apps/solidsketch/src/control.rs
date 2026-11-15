//! The control channel (docs/ARCHITECTURE.md §7 quater, §7 septies), desktop only: the
//! server of `solidsketch-control` (loopback HTTP, a token per start) as the app holds it,
//! switched on and off while it runs — to drive and inspect the running app from a script or
//! an agent, like the Claude Bridge does SketchUp.
//!
//! Off by default: switched on and off while the app runs ([`Remote`], the command
//! `app.remote_control`, its preference), or at launch by `SOLIDSKETCH_CONTROL=1`. Its port:
//! a preference (default 7858), `SOLIDSKETCH_CONTROL_PORT` for one launch. The title bar
//! shows it while it is on. The methods and the routes: `solidsketch_control::server`; the
//! app answers them at its next frame (`SolidSketchApp::serve_control`).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use solidsketch_control::server::Control;

/// The environment asks for the channel at launch (`SOLIDSKETCH_CONTROL=1`).
pub fn env_wanted() -> bool {
    std::env::var("SOLIDSKETCH_CONTROL").ok().as_deref() == Some("1")
}

/// The port the environment asks for at launch (`SOLIDSKETCH_CONTROL_PORT`).
pub fn env_port() -> Option<u16> {
    std::env::var("SOLIDSKETCH_CONTROL_PORT").ok().and_then(|p| p.parse().ok())
}

/// How long the title bar's mark flashes after a request.
pub const FLASH: Duration = Duration::from_millis(600);

/// The channel as the app holds it: on (running, or failed to start: the user asked for it
/// and the title bar says why it is not running) or off.
pub struct Remote {
    control: Option<Control>,
    error: Option<String>,
    port: u16,
    /// Where its token and discovery files go (none: not written, tests).
    dir: Option<PathBuf>,
    wake: Arc<dyn Fn() + Send + Sync>,
    /// The served count last seen, and when it last grew.
    seen: u64,
    last: Option<Instant>,
}

/// What the title bar shows of it.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Off,
    On {
        port: u16,
        served: u64,
        /// 1 just after a request, down to 0 after [`FLASH`].
        flash: f32,
        /// Its token file, if written.
        token_file: Option<PathBuf>,
    },
    Failed {
        port: u16,
        error: String,
    },
}

impl Remote {
    pub fn new(port: u16, dir: Option<PathBuf>, wake: impl Fn() + Send + Sync + 'static) -> Self {
        Remote { control: None, error: None, port, dir, wake: Arc::new(wake), seen: 0, last: None }
    }

    /// Asked for: running, or failed to start.
    pub fn is_on(&self) -> bool {
        self.control.is_some() || self.error.is_some()
    }

    /// Switches it on (again: a failed start is tried again) or off. The error, if it fails.
    pub fn set(&mut self, on: bool) -> Result<(), String> {
        self.control = None;
        self.error = None;
        if !on {
            return Ok(());
        }
        let wake = self.wake.clone();
        match Control::start(self.port, self.dir.as_deref(), move || wake()) {
            Ok(c) => {
                log::info!("control channel on 127.0.0.1:{}", c.port);
                self.seen = c.served();
                self.control = Some(c);
                Ok(())
            }
            Err(e) => {
                self.error = Some(e.clone());
                Err(e)
            }
        }
    }

    /// Moves it to `port`: started again there only if it is on and the port changes.
    pub fn set_port(&mut self, port: u16) -> Result<(), String> {
        if port == self.port {
            return Ok(());
        }
        self.port = port;
        if self.is_on() { self.set(true) } else { Ok(()) }
    }

    pub fn control(&self) -> Option<&Control> {
        self.control.as_ref()
    }

    /// Its state now; notes the requests served since the last call.
    pub fn status(&mut self) -> Status {
        if let Some(e) = &self.error {
            return Status::Failed { port: self.port, error: e.clone() };
        }
        let Some(c) = &self.control else { return Status::Off };
        let served = c.served();
        if served != self.seen {
            self.seen = served;
            self.last = Some(Instant::now());
        }
        let flash = self.last.map_or(0.0, |t| 1.0 - (t.elapsed().as_secs_f32() / FLASH.as_secs_f32()).min(1.0));
        Status::On { port: c.port, served, flash, token_file: c.token_file() }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    use super::*;

    fn ping(port: u16, token: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let body = "{\"method\":\"app.ping\"}";
        let head =
            format!("POST /call HTTP/1.1\r\nX-SolidSketch: 1\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\n\r\n", body.len());
        s.write_all(format!("{head}{body}").as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    }

    #[test]
    fn stopping_frees_the_port_and_starting_again_works() {
        let mut remote = Remote::new(0, None, || {});
        remote.set(true).unwrap();
        let (port, token) = (remote.control().unwrap().port, remote.control().unwrap().token().to_owned());
        let answer = ping(port, &token);
        assert!(answer.contains("\"ok\":true"), "{answer}");
        assert!(matches!(remote.status(), Status::On { served: 1, .. }));
        remote.set(false).unwrap();
        assert_eq!(remote.status(), Status::Off);
        // The listener is gone: the port can be taken again.
        let mut again = None;
        for _ in 0..50 {
            if let Ok(c) = Control::start(port, None, || {}) {
                again = Some(c);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(again.map(|c| c.port), Some(port));
    }

    #[test]
    fn a_port_taken_leaves_it_failed_but_on() {
        let first = Control::start(0, None, || {}).unwrap();
        let mut remote = Remote::new(first.port, None, || {});
        assert!(remote.set(true).is_err());
        assert!(remote.is_on());
        assert!(matches!(remote.status(), Status::Failed { .. }));
        drop(first);
        remote.set(false).unwrap();
        assert!(!remote.is_on());
    }

    #[test]
    fn a_new_port_restarts_it_only_when_on_and_changed() {
        let mut remote = Remote::new(0, None, || {});
        remote.set(true).unwrap();
        let first = remote.control().unwrap().port;
        // Port 0 again: the same, nothing restarted.
        remote.set_port(0).unwrap();
        assert_eq!(remote.control().unwrap().port, first);
        // Off: the port is only noted.
        remote.set(false).unwrap();
        let taken = Control::start(0, None, || {}).unwrap();
        remote.set_port(taken.port).unwrap();
        assert!(!remote.is_on());
        // On, to a port taken: failed, still on; back to a free one: running.
        remote.set_port(0).unwrap();
        remote.set(true).unwrap();
        assert!(remote.set_port(taken.port).is_err());
        assert!(matches!(remote.status(), Status::Failed { .. }));
        remote.set_port(0).unwrap();
        assert!(matches!(remote.status(), Status::On { .. }));
    }
}

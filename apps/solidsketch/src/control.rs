//! The control channel (docs/ARCHITECTURE.md §7 quater), desktop only: a small HTTP server on
//! the loopback interface that drives the running app through the command registry — to
//! test the real interface from a script, like the Claude Bridge does SketchUp.
//!
//! Off by default: switched on and off while the app runs ([`Remote`], the command
//! `app.remote_control`, its preference), or at launch by `SOLIDSKETCH_CONTROL=1`. Its port:
//! a preference (default 7858), `SOLIDSKETCH_CONTROL_PORT` for one launch. The title bar
//! shows it while it is on. Every
//! request needs the header `X-SolidSketch: 1` (a page in a browser cannot send it without a
//! preflight the server never answers). Requests are handed to the UI thread, which answers
//! at its next frame:
//!
//! - `GET /ping` → `{"ok":true}`
//! - `GET /commands` → the commands, enabled or not, checked, shortcut
//! - `POST /run` with a command id as body → runs it
//! - `GET /state` → tool, selection, status, history revision, modified
//! - `GET /screenshot` → the viewport, as a PNG
//! - `GET /docs` → the open documents (name, path, modified), and the shown one
//! - `POST /docs/switch` with a tab's index as body → shows it
//! - `POST /docs/new` → a new document, shown
//! - `POST /docs/close` with a tab's index as body, `force` after it to skip the save
//!   question → closes it (else asks, as the tab's ×)

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

const HEADER: &str = "x-solidsketch";

/// What a client asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Commands,
    Run(String),
    State,
    Screenshot,
    Docs,
    DocsSwitch(usize),
    DocsNew,
    DocsClose { index: usize, force: bool },
}

/// The UI's answer: a status, a content type and a body.
pub struct Reply {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn json(status: u16, value: serde_json::Value) -> Self {
        Reply { status, content_type: "application/json", body: value.to_string().into_bytes() }
    }
}

/// A request waiting for the UI, and where its reply goes.
pub struct Pending {
    pub request: Request,
    reply: mpsc::Sender<Reply>,
}

impl Pending {
    pub fn answer(self, reply: Reply) {
        let _ = self.reply.send(reply);
    }
}

/// The server's end on the UI side. Dropping it stops the server and frees its port.
pub struct Control {
    inbox: mpsc::Receiver<Pending>,
    pub port: u16,
    stop: Arc<AtomicBool>,
    /// The requests answered or handed to the UI so far (the guard's refusals not counted).
    served: Arc<AtomicU64>,
}

/// The environment asks for the channel at launch (`SOLIDSKETCH_CONTROL=1`).
pub fn env_wanted() -> bool {
    std::env::var("SOLIDSKETCH_CONTROL").ok().as_deref() == Some("1")
}

/// The port the environment asks for at launch (`SOLIDSKETCH_CONTROL_PORT`).
pub fn env_port() -> Option<u16> {
    std::env::var("SOLIDSKETCH_CONTROL_PORT").ok().and_then(|p| p.parse().ok())
}

impl Control {
    pub fn start(port: u16, wake: impl Fn() + Send + Sync + 'static) -> Result<Control, String> {
        let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("127.0.0.1:{port} : {e}"))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let (tx, inbox) = mpsc::channel();
        let wake = Arc::new(wake);
        let stop = Arc::new(AtomicBool::new(false));
        let served = Arc::new(AtomicU64::new(0));
        let (stopped, count) = (stop.clone(), served.clone());
        std::thread::Builder::new()
            .name("control".into())
            .spawn(move || {
                // Ends at the first connection after the stop ([`Control::drop`] makes one): the
                // listener goes with the thread.
                for stream in listener.incoming().flatten() {
                    if stopped.load(Ordering::Acquire) {
                        break;
                    }
                    let (tx, wake, count) = (tx.clone(), wake.clone(), count.clone());
                    std::thread::spawn(move || serve(stream, &tx, &*wake, &count));
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Control { inbox, port, stop, served })
    }

    /// The requests that came since the last frame.
    pub fn take(&self) -> Vec<Pending> {
        self.inbox.try_iter().collect()
    }

    pub fn served(&self) -> u64 {
        self.served.load(Ordering::Relaxed)
    }
}

impl Drop for Control {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Wakes the listener blocked in `accept`. The requests on their way get a 503: the
        // inbox is gone.
        let _ = TcpStream::connect_timeout(&(std::net::Ipv4Addr::LOCALHOST, self.port).into(), Duration::from_millis(200));
    }
}

/// How long the title bar's mark flashes after a request.
pub const FLASH: Duration = Duration::from_millis(600);

/// The channel as the app holds it: on (running, or failed to start: the user asked for it
/// and the title bar says why it is not running) or off.
pub struct Remote {
    control: Option<Control>,
    error: Option<String>,
    port: u16,
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
    },
    Failed {
        port: u16,
        error: String,
    },
}

impl Remote {
    pub fn new(port: u16, wake: impl Fn() + Send + Sync + 'static) -> Self {
        Remote { control: None, error: None, port, wake: Arc::new(wake), seen: 0, last: None }
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
        match Control::start(self.port, move || wake()) {
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
        Status::On { port: c.port, served, flash }
    }
}

fn serve(mut stream: TcpStream, tx: &mpsc::Sender<Pending>, wake: &dyn Fn(), served: &AtomicU64) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let request = read_request(&stream);
    if request.is_ok() {
        served.fetch_add(1, Ordering::Relaxed);
        // The title bar flashes.
        wake();
    }
    let reply = match request {
        Ok(r) => match r {
            Parsed::Ping => Reply::json(200, serde_json::json!({ "ok": true })),
            Parsed::Request(request) => {
                let (reply, rx) = mpsc::channel();
                if tx.send(Pending { request, reply }).is_err() {
                    Reply::json(503, serde_json::json!({ "ok": false, "error": "app closed" }))
                } else {
                    wake();
                    rx.recv_timeout(Duration::from_secs(10))
                        .unwrap_or_else(|_| Reply::json(504, serde_json::json!({ "ok": false, "error": "no answer from the UI" })))
                }
            }
        },
        Err((status, error)) => Reply::json(status, serde_json::json!({ "ok": false, "error": error })),
    };
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reply.status,
        if reply.status < 300 { "OK" } else { "Error" },
        reply.content_type,
        reply.body.len()
    );
    let _ = stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(&reply.body));
}

enum Parsed {
    Ping,
    Request(Request),
}

/// Reads one request: its line, its headers (the guard among them) and its body.
fn read_request(stream: &TcpStream) -> Result<Parsed, (u16, String)> {
    let mut r = BufReader::new(stream);
    let mut line = String::new();
    r.read_line(&mut line).map_err(|e| (400, e.to_string()))?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let mut guarded = false;
    let mut length = 0usize;
    loop {
        let mut h = String::new();
        r.read_line(&mut h).map_err(|e| (400, e.to_string()))?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((name, value)) = h.split_once(':') {
            let (name, value) = (name.trim().to_ascii_lowercase(), value.trim());
            if name == HEADER && value == "1" {
                guarded = true;
            }
            if name == "content-length" {
                length = value.parse().unwrap_or(0);
            }
        }
    }
    if !guarded {
        return Err((403, format!("the {HEADER}: 1 header is required")));
    }
    let mut body = vec![0; length.min(1 << 16)];
    r.read_exact(&mut body).map_err(|e| (400, e.to_string()))?;
    parse(method, path, &String::from_utf8_lossy(&body))
}

fn parse(method: &str, path: &str, body: &str) -> Result<Parsed, (u16, String)> {
    Ok(match (method, path) {
        ("GET", "/ping") => Parsed::Ping,
        ("GET", "/commands") => Parsed::Request(Request::Commands),
        ("POST", "/run") => Parsed::Request(Request::Run(body.trim().to_owned())),
        ("GET", "/state") => Parsed::Request(Request::State),
        ("GET", "/screenshot") => Parsed::Request(Request::Screenshot),
        ("GET", "/docs") => Parsed::Request(Request::Docs),
        ("POST", "/docs/new") => Parsed::Request(Request::DocsNew),
        ("POST", "/docs/switch") => Parsed::Request(Request::DocsSwitch(index(body)?)),
        ("POST", "/docs/close") => {
            let force = body.split_whitespace().nth(1) == Some("force");
            Parsed::Request(Request::DocsClose { index: index(body)?, force })
        }
        _ => return Err((404, format!("no {method} {path}"))),
    })
}

/// The tab index a body starts with.
fn index(body: &str) -> Result<usize, (u16, String)> {
    body.split_whitespace().next().and_then(|t| t.parse().ok()).ok_or((400, format!("not a tab index: {body:?}")))
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

    fn send(port: u16, text: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(text.as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    }

    #[test]
    fn requests_need_the_header_and_reach_the_ui() {
        let control = Control::start(0, || {}).unwrap();
        let port = control.port;
        // The "UI": answers what comes, as a frame would.
        let ui = std::thread::spawn(move || {
            let mut answered = 0;
            while answered < 1 {
                for p in control.take() {
                    assert_eq!(p.request, Request::Run("tool.move".into()));
                    p.answer(Reply::json(200, serde_json::json!({ "ok": true, "ran": "tool.move" })));
                    answered += 1;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        let refused = send(port, "GET /ping HTTP/1.1\r\n\r\n");
        assert!(refused.starts_with("HTTP/1.1 403"), "{refused}");
        let ping = send(port, "GET /ping HTTP/1.1\r\nX-SolidSketch: 1\r\n\r\n");
        assert!(ping.contains("{\"ok\":true}"), "{ping}");
        let missing = send(port, "GET /nope HTTP/1.1\r\nX-SolidSketch: 1\r\n\r\n");
        assert!(missing.starts_with("HTTP/1.1 404"), "{missing}");
        let run = send(port, "POST /run HTTP/1.1\r\nx-solidsketch: 1\r\nContent-Length: 9\r\n\r\ntool.move");
        assert!(run.contains("\"ran\":\"tool.move\""), "{run}");
        ui.join().unwrap();
    }

    #[test]
    fn stopping_frees_the_port_and_starting_again_works() {
        let mut remote = Remote::new(0, || {});
        remote.set(true).unwrap();
        let port = remote.control().unwrap().port;
        let ping = send(port, "GET /ping HTTP/1.1\r\nX-SolidSketch: 1\r\n\r\n");
        assert!(ping.contains("{\"ok\":true}"), "{ping}");
        assert!(matches!(remote.status(), Status::On { served: 1, .. }));
        remote.set(false).unwrap();
        assert_eq!(remote.status(), Status::Off);
        // The listener is gone: the port can be taken again.
        let mut again = None;
        for _ in 0..50 {
            if let Ok(c) = Control::start(port, || {}) {
                again = Some(c);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(again.map(|c| c.port), Some(port));
    }

    #[test]
    fn a_port_taken_leaves_it_failed_but_on() {
        let first = Control::start(0, || {}).unwrap();
        let mut remote = Remote::new(first.port, || {});
        assert!(remote.set(true).is_err());
        assert!(remote.is_on());
        assert!(matches!(remote.status(), Status::Failed { .. }));
        drop(first);
        remote.set(false).unwrap();
        assert!(!remote.is_on());
    }

    #[test]
    fn a_new_port_restarts_it_only_when_on_and_changed() {
        let mut remote = Remote::new(0, || {});
        remote.set(true).unwrap();
        let first = remote.control().unwrap().port;
        // Port 0 again: the same, nothing restarted.
        remote.set_port(0).unwrap();
        assert_eq!(remote.control().unwrap().port, first);
        // Off: the port is only noted.
        remote.set(false).unwrap();
        let taken = Control::start(0, || {}).unwrap();
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

    #[test]
    fn a_screenshot_is_a_png() {
        let bytes = png(2, 1, &[255, 0, 0, 255, 0, 255, 0, 255]).unwrap();
        assert_eq!(&bytes[1..4], b"PNG");
    }
}

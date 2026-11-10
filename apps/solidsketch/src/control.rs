//! The control channel (docs/ARCHITECTURE.md §7 quater), desktop only: a small HTTP server on
//! the loopback interface that drives the running app through the command registry — to
//! test the real interface from a script, like the Claude Bridge does SketchUp.
//!
//! Off unless `SOLIDSKETCH_CONTROL=1` (port `SOLIDSKETCH_CONTROL_PORT`, default 7858). Every
//! request needs the header `X-SolidSketch: 1` (a page in a browser cannot send it without a
//! preflight the server never answers). Requests are handed to the UI thread, which answers
//! at its next frame:
//!
//! - `GET /ping` → `{"ok":true}`
//! - `GET /commands` → the commands, enabled or not, checked, shortcut
//! - `POST /run` with a command id as body → runs it
//! - `GET /state` → tool, selection, status, history revision, modified
//! - `GET /screenshot` → the viewport, as a PNG

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

pub const DEFAULT_PORT: u16 = 7858;
const HEADER: &str = "x-solidsketch";

/// What a client asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Commands,
    Run(String),
    State,
    Screenshot,
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

/// The server's end on the UI side.
pub struct Control {
    inbox: mpsc::Receiver<Pending>,
    pub port: u16,
}

impl Control {
    /// Starts the server when the environment asks for it; `wake` makes the UI draw a frame.
    pub fn from_env(wake: impl Fn() + Send + Sync + 'static) -> Option<Result<Control, String>> {
        if std::env::var("SOLIDSKETCH_CONTROL").ok().as_deref() != Some("1") {
            return None;
        }
        let port = std::env::var("SOLIDSKETCH_CONTROL_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT);
        Some(Control::start(port, wake))
    }

    pub fn start(port: u16, wake: impl Fn() + Send + Sync + 'static) -> Result<Control, String> {
        let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("127.0.0.1:{port} : {e}"))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let (tx, inbox) = mpsc::channel();
        let wake = std::sync::Arc::new(wake);
        std::thread::Builder::new()
            .name("control".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let (tx, wake) = (tx.clone(), wake.clone());
                    std::thread::spawn(move || serve(stream, &tx, &*wake));
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Control { inbox, port })
    }

    /// The requests that came since the last frame.
    pub fn take(&self) -> Vec<Pending> {
        self.inbox.try_iter().collect()
    }
}

fn serve(mut stream: TcpStream, tx: &mpsc::Sender<Pending>, wake: &dyn Fn()) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let reply = match read_request(&stream) {
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
        _ => return Err((404, format!("no {method} {path}"))),
    })
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
    fn a_screenshot_is_a_png() {
        let bytes = png(2, 1, &[255, 0, 0, 255, 0, 255, 0, 255]).unwrap();
        assert_eq!(&bytes[1..4], b"PNG");
    }
}

//! The loopback HTTP server of the desktop (docs/ARCHITECTURE.md §7 septies): it admits a
//! request by its token and hands it to the UI thread, which answers at its next frame.
//!
//! One route: `POST /call` with `{"method": "…", "params": {…}}` as body, any method. The
//! reply is `{"ok": true, "result": …}` (an image for `app.screenshot`) or `{"ok": false,
//! "error": "…"}` with a status: 400 bad parameters, 404 no such method, 422 refused, 503
//! unavailable. `app.ping` is answered here, without the UI.
//!
//! **Token**: each start makes a random 256-bit token (or takes `SKRAFT_CONTROL_TOKEN`)
//! and writes it, as one line of hex, to `<dir>/<port>.token`, readable by the user alone, and
//! the port to `<dir>/current.port`, where a client finds the running app; both are removed
//! when it stops. `<dir>` is `Skraft/control` in the user's data folder ([`default_dir`]).
//! Every request sends `Authorization: Bearer <token>` and `X-Skraft: 1` (a page in a
//! browser cannot send the latter without a preflight the server never answers); it may name
//! its client with `X-Skraft-Client: <name>` (at most [`MAX_CLIENT`] characters), which
//! the journal shows for what it caused.
//!
//! Several apps may run (one per worktree): each one says what it is in its own file of the
//! registry ([`crate::instances`]), where a client finds the one it wants.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use serde_json::{Value, json};

use crate::params::WaitParams;
use crate::{Answer, Call, Error, Outcome};

const GUARD: &str = "x-skraft";
const CLIENT: &str = "x-skraft-client";
/// The longest client's name kept.
pub const MAX_CLIENT: usize = 80;
/// The largest body read (a batch of operations, later).
const MAX_BODY: usize = 1 << 20;
/// How long a request waits for the UI, but `app.wait` (its own timeout, and some slack).
const UI_TIMEOUT: Duration = Duration::from_secs(60);

/// The folder of the token and discovery files: `Skraft/control` in the user's data
/// folder (`~/Library/Application Support` on macOS, `~/.local/share` on Linux, `%APPDATA%`
/// on Windows).
pub fn default_dir() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("Skraft").join("control"))
}

/// The token file of the server on `port`.
pub fn token_path(dir: &Path, port: u16) -> PathBuf {
    dir.join(format!("{port}.token"))
}

/// The discovery file: the port of the server started last.
pub fn port_path(dir: &Path) -> PathBuf {
    dir.join("current.port")
}

/// A request waiting for the UI, and where its answer goes.
pub struct Pending {
    pub call: Call,
    reply: mpsc::Sender<Outcome>,
}

impl Pending {
    pub fn answer(self, outcome: Outcome) {
        let _ = self.reply.send(outcome);
    }
}

/// The server's end on the UI side. Dropping it stops the server, frees its port and removes
/// its files.
pub struct Control {
    inbox: mpsc::Receiver<Pending>,
    pub port: u16,
    token: String,
    /// The folder its files are in, if written.
    dir: Option<PathBuf>,
    stop: Arc<AtomicBool>,
    /// The requests admitted so far (refused ones not counted).
    served: Arc<AtomicU64>,
}

impl Control {
    /// Starts it on `port` (0: any free one), its files in `dir` (none: no files, the token
    /// given some other way). `wake` asks the UI for a frame.
    pub fn start(port: u16, dir: Option<&Path>, wake: impl Fn() + Send + Sync + 'static) -> Result<Control, String> {
        let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("127.0.0.1:{port} : {e}"))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let token = match std::env::var("SKRAFT_CONTROL_TOKEN") {
            Ok(t) if !t.trim().is_empty() => t.trim().to_owned(),
            _ => new_token()?,
        };
        if let Some(dir) = dir {
            write_files(dir, port, &token).map_err(|e| format!("{} : {e}", dir.display()))?;
        }
        let (tx, inbox) = mpsc::channel();
        let wake = Arc::new(wake);
        let stop = Arc::new(AtomicBool::new(false));
        let served = Arc::new(AtomicU64::new(0));
        let (stopped, count, expected) = (stop.clone(), served.clone(), Arc::new(token.clone()));
        std::thread::Builder::new()
            .name("control".into())
            .spawn(move || {
                // Ends at the first connection after the stop ([`Control::drop`] makes one): the
                // listener goes with the thread.
                for stream in listener.incoming().flatten() {
                    if stopped.load(Ordering::Acquire) {
                        break;
                    }
                    let (tx, wake, count, expected) = (tx.clone(), wake.clone(), count.clone(), expected.clone());
                    std::thread::spawn(move || serve(stream, &tx, &*wake, &count, &expected));
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Control { inbox, port, token, dir: dir.map(Path::to_path_buf), stop, served })
    }

    /// The requests that came since the last frame.
    pub fn take(&self) -> Vec<Pending> {
        self.inbox.try_iter().collect()
    }

    pub fn served(&self) -> u64 {
        self.served.load(Ordering::Relaxed)
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// Its token file, if written.
    pub fn token_file(&self) -> Option<PathBuf> {
        self.dir.as_deref().map(|d| token_path(d, self.port))
    }
}

impl Drop for Control {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(dir) = &self.dir {
            remove_files(dir, self.port, &self.token);
        }
        // Wakes the listener blocked in `accept`. The requests on their way get a 503: the
        // inbox is gone.
        let _ = TcpStream::connect_timeout(&(std::net::Ipv4Addr::LOCALHOST, self.port).into(), Duration::from_millis(200));
    }
}

/// 32 random bytes, in hex.
fn new_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Writes `text` to `path`, readable and writable by the user alone.
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    let mut f = o.open(path)?;
    // A file that was there keeps its mode: set it again.
    #[cfg(unix)]
    std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    f.write_all(text.as_bytes())?;
    f.write_all(b"\n")
}

fn write_files(dir: &Path, port: u16, token: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    write_private(&token_path(dir, port), token)?;
    write_private(&port_path(dir), &port.to_string())
}

/// Removes the files of the server on `port`, unless another one wrote them since.
fn remove_files(dir: &Path, port: u16, token: &str) {
    let read = |p: &Path| std::fs::read_to_string(p).ok().map(|t| t.trim().to_owned());
    let token_file = token_path(dir, port);
    if read(&token_file).as_deref() == Some(token) {
        let _ = std::fs::remove_file(token_file);
    }
    let port_file = port_path(dir);
    if read(&port_file) == Some(port.to_string()) {
        let _ = std::fs::remove_file(port_file);
    }
}

/// A request as read: a call for the UI, or a ping answered here.
#[derive(Debug, PartialEq)]
enum Parsed {
    Ping,
    Call(Call),
}

/// An HTTP answer.
struct Response {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

impl Response {
    fn json(status: u16, value: Value) -> Self {
        Response { status, content_type: "application/json", body: value.to_string().into_bytes() }
    }

    fn of(outcome: Outcome) -> Self {
        match outcome {
            Ok(Answer::Json(v)) => Response::json(200, json!({ "ok": true, "result": v })),
            Ok(Answer::Png(bytes)) => Response { status: 200, content_type: "image/png", body: bytes },
            Err(e) => Response::json(e.status(), json!({ "ok": false, "error": e.message() })),
        }
    }
}

fn serve(mut stream: TcpStream, tx: &mpsc::Sender<Pending>, wake: &dyn Fn(), served: &AtomicU64, token: &str) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let request = read_request(&stream, token);
    if request.is_ok() {
        served.fetch_add(1, Ordering::Relaxed);
        // The title bar flashes.
        wake();
    }
    let response = match request {
        Ok(Parsed::Ping) => Response::of(Ok(Answer::Json(json!({})))),
        Ok(Parsed::Call(call)) => {
            let timeout = match call.method.as_str() {
                "app.wait" => call.params::<WaitParams>().map_or(UI_TIMEOUT, |w| w.timeout() + Duration::from_secs(5)),
                _ => UI_TIMEOUT,
            };
            let (reply, rx) = mpsc::channel();
            if tx.send(Pending { call, reply }).is_err() {
                Response::of(Err(Error::Unavailable("app closed".into())))
            } else {
                wake();
                rx.recv_timeout(timeout)
                    .map_or_else(|_| Response::json(504, json!({ "ok": false, "error": "no answer from the UI" })), Response::of)
            }
        }
        Err((status, error)) => Response::json(status, json!({ "ok": false, "error": error })),
    };
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        if response.status < 300 { "OK" } else { "Error" },
        response.content_type,
        response.body.len()
    );
    let _ = stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(&response.body));
}

/// Reads one request: its line, its headers (the guard and the token among them) and its body.
fn read_request(stream: &TcpStream, token: &str) -> Result<Parsed, (u16, String)> {
    let mut r = BufReader::new(stream);
    let mut line = String::new();
    r.read_line(&mut line).map_err(|e| (400, e.to_string()))?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let (mut guarded, mut authorized) = (false, false);
    let mut length = 0usize;
    let mut client = None;
    loop {
        let mut h = String::new();
        r.read_line(&mut h).map_err(|e| (400, e.to_string()))?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((name, value)) = h.split_once(':') {
            let (name, value) = (name.trim().to_ascii_lowercase(), value.trim());
            match name.as_str() {
                GUARD => guarded = value == "1",
                "authorization" => authorized = value.strip_prefix("Bearer ").is_some_and(|t| same(t.trim(), token)),
                "content-length" => length = value.parse().unwrap_or(0),
                CLIENT if !value.is_empty() => client = Some(value.chars().take(MAX_CLIENT).collect()),
                _ => {}
            }
        }
    }
    if length > MAX_BODY {
        return Err((413, format!("a body of at most {MAX_BODY} bytes")));
    }
    // Read even when refused: a socket closed with bytes unread is reset, and the client
    // gets the reset, not the answer.
    let mut body = vec![0; length];
    r.read_exact(&mut body).map_err(|e| (400, e.to_string()))?;
    if !guarded {
        return Err((403, format!("the {GUARD}: 1 header is required")));
    }
    if !authorized {
        return Err((401, "the token is required: Authorization: Bearer <the token file's content>".into()));
    }
    match parse(method, path, &String::from_utf8_lossy(&body))? {
        Parsed::Call(call) => Ok(Parsed::Call(call.from_client(client))),
        ping => Ok(ping),
    }
}

/// Compares tokens in a time that does not tell how much of them matched.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn parse(method: &str, path: &str, body: &str) -> Result<Parsed, (u16, String)> {
    match (method, path) {
        ("POST", "/call") => {
            let v: Value = serde_json::from_str(body).map_err(|e| (400, format!("not JSON: {e}")))?;
            let Some(name) = v.get("method").and_then(Value::as_str) else {
                return Err((400, "the body is {\"method\": \"…\", \"params\": {…}}".into()));
            };
            if let Some(extra) = v.as_object().and_then(|o| o.keys().find(|k| *k != "method" && *k != "params")) {
                return Err((400, format!("unknown field {extra:?}: the body is {{\"method\", \"params\"}}")));
            }
            if name == "app.ping" {
                return Ok(Parsed::Ping);
            }
            Ok(Parsed::Call(Call::new(name, v.get("params").cloned().unwrap_or(Value::Null))))
        }
        _ => Err((404, format!("no {method} {path}: POST /call with {{\"method\", \"params\"}}"))),
    }
}

// ---- client ------------------------------------------------------------------------------------

/// Where a running app's channel is: its port and token, from the discovery and token files in
/// `dir` (or `port` given; `SKRAFT_CONTROL_TOKEN` for the token).
pub fn discover(dir: &Path, port: Option<u16>) -> Result<(u16, String), String> {
    let port = match port {
        Some(p) => p,
        None => std::fs::read_to_string(port_path(dir))
            .ok()
            .and_then(|t| t.trim().parse().ok())
            .ok_or_else(|| format!("no running Skraft with its control channel on (no {})", port_path(dir).display()))?,
    };
    let token = match std::env::var("SKRAFT_CONTROL_TOKEN") {
        Ok(t) if !t.trim().is_empty() => t.trim().to_owned(),
        _ => std::fs::read_to_string(token_path(dir, port))
            .map(|t| t.trim().to_owned())
            .map_err(|_| format!("no token for port {port} (no {})", token_path(dir, port).display()))?,
    };
    Ok((port, token))
}

/// Calls `call` on the app listening on `port` (the loopback interface), with its `token`;
/// `call.client` names the caller.
pub fn call(port: u16, token: &str, call: &Call) -> Outcome {
    let body = json!({ "method": call.method, "params": call.params }).to_string();
    let unreachable = |e: std::io::Error| {
        Error::Unavailable(format!(
            "no Skraft app answers at 127.0.0.1:{port} ({e}): start it with its control channel on (title bar, or SKRAFT_CONTROL=1)"
        ))
    };
    let mut stream =
        TcpStream::connect_timeout(&(std::net::Ipv4Addr::LOCALHOST, port).into(), Duration::from_secs(2)).map_err(unreachable)?;
    let timeout = match call.method.as_str() {
        "app.wait" => call.params::<WaitParams>().map_or(UI_TIMEOUT, |w| w.timeout() + Duration::from_secs(10)),
        _ => UI_TIMEOUT + Duration::from_secs(5),
    };
    let _ = stream.set_read_timeout(Some(timeout));
    // One line: no line break smuggled into the headers.
    let client = call.client.as_deref().map_or_else(String::new, |c| {
        format!("X-Skraft-Client: {}\r\n", c.chars().filter(|c| !c.is_control()).take(MAX_CLIENT).collect::<String>())
    });
    let head = format!(
        "POST /call HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Skraft: 1\r\nAuthorization: Bearer {token}\r\n{client}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(body.as_bytes())).map_err(unreachable)?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(unreachable)?;
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| Error::Failed("a reply without headers".into()))?;
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let body = &raw[split + 4..];
    let status: u16 = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    if head.to_ascii_lowercase().contains("content-type: image/png") {
        return Ok(Answer::Png(body.to_vec()));
    }
    let v: Value = serde_json::from_slice(body).map_err(|e| Error::Failed(format!("a reply not JSON ({e})")))?;
    if status == 200 && v["ok"] == true {
        return Ok(Answer::Json(v.get("result").cloned().unwrap_or(Value::Null)));
    }
    let message = v["error"].as_str().unwrap_or("refused").to_owned();
    Err(match status {
        400 => Error::BadParams(message),
        503 => Error::Unavailable(message),
        _ => Error::Failed(message),
    })
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

    fn post(port: u16, token: &str, path: &str, body: &str) -> String {
        send(
            port,
            &format!(
                "POST {path} HTTP/1.1\r\nX-Skraft: 1\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        )
    }

    /// A folder of its own under the system's temporary one.
    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("skraft-control-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn requests_need_the_guard_and_the_token_and_reach_the_ui() {
        let control = Control::start(0, None, || {}).unwrap();
        let (port, token) = (control.port, control.token().to_owned());
        assert_eq!(token.len(), 64);
        // The "UI": answers what comes, as a frame would.
        let ui = std::thread::spawn(move || {
            let mut answered = 0;
            while answered < 2 {
                for p in control.take() {
                    let outcome = match p.call.method.as_str() {
                        "app.run" => Ok(Answer::Json(json!({ "ran": p.call.params["id"], "client": p.call.client }))),
                        m => Err(Error::UnknownMethod(m.into())),
                    };
                    p.answer(outcome);
                    answered += 1;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        let ping = "{\"method\":\"app.ping\"}";
        let unguarded =
            send(port, &format!("POST /call HTTP/1.1\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\n\r\n{ping}", ping.len()));
        assert!(unguarded.starts_with("HTTP/1.1 403"), "{unguarded}");
        let no_token = send(port, &format!("POST /call HTTP/1.1\r\nX-Skraft: 1\r\nContent-Length: {}\r\n\r\n{ping}", ping.len()));
        assert!(no_token.starts_with("HTTP/1.1 401"), "{no_token}");
        let wrong = post(port, "00", "/call", ping);
        assert!(wrong.starts_with("HTTP/1.1 401"), "{wrong}");
        let pong = send(
            port,
            &format!("POST /call HTTP/1.1\r\nx-skraft: 1\r\nauthorization: Bearer {token}\r\ncontent-length: {}\r\n\r\n{ping}", ping.len()),
        );
        assert!(pong.contains("\"ok\":true"), "{pong}");
        // The one route.
        let old = post(port, &token, "/run", "tool.move");
        assert!(old.starts_with("HTTP/1.1 404"), "{old}");
        let body = "{\"method\":\"app.run\",\"params\":{\"id\":\"tool.move\"}}";
        let run = send(
            port,
            &format!(
                "POST /call HTTP/1.1\r\nX-Skraft: 1\r\nX-Skraft-Client: mcp · test\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        );
        assert!(run.contains("\"ran\":\"tool.move\"") && run.contains("\"client\":\"mcp · test\""), "{run}");
        let unknown = post(port, &token, "/call", "{\"method\":\"tool.nope\"}");
        assert!(unknown.starts_with("HTTP/1.1 404") && unknown.contains("tool.nope"), "{unknown}");
        // Refused before the UI.
        let not_json = post(port, &token, "/call", "tool.move");
        assert!(not_json.starts_with("HTTP/1.1 400"), "{not_json}");
        let extra = post(port, &token, "/call", "{\"method\":\"app.state\",\"param\":{}}");
        assert!(extra.starts_with("HTTP/1.1 400") && extra.contains("param"), "{extra}");
        ui.join().unwrap();
    }

    #[test]
    fn its_files_are_private_and_go_with_it() {
        let dir = temp_dir("files");
        let control = Control::start(0, Some(&dir), || {}).unwrap();
        let token_file = control.token_file().unwrap();
        assert_eq!(std::fs::read_to_string(&token_file).unwrap().trim(), control.token());
        assert_eq!(std::fs::read_to_string(port_path(&dir)).unwrap().trim(), control.port.to_string());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&token_file).unwrap().permissions().mode() & 0o777, 0o600);
            assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
        }
        // A second server started since: the discovery file is its, and stays.
        let second = Control::start(0, Some(&dir), || {}).unwrap();
        drop(control);
        assert!(!token_file.exists());
        assert_eq!(std::fs::read_to_string(port_path(&dir)).unwrap().trim(), second.port.to_string());
        drop(second);
        assert!(!port_path(&dir).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_call_is_a_method_and_its_params() {
        assert_eq!(
            parse("POST", "/call", "{\"method\":\"app.docs\",\"params\":{\"action\":\"new\"}}"),
            Ok(Parsed::Call(Call::new("app.docs", json!({ "action": "new" }))))
        );
        assert_eq!(parse("POST", "/call", "{\"method\":\"app.state\"}"), Ok(Parsed::Call(Call::new("app.state", Value::Null))));
        assert_eq!(parse("POST", "/call", "{\"method\":\"app.ping\"}"), Ok(Parsed::Ping));
        assert!(parse("GET", "/state", "").is_err());
        assert!(parse("POST", "/call", "{\"params\":{}}").is_err());
    }

    #[test]
    fn the_client_finds_the_app_and_calls_it() {
        let dir = temp_dir("client");
        let control = Control::start(0, Some(&dir), || {}).unwrap();
        let (port, token) = discover(&dir, None).unwrap();
        assert_eq!((port, token.as_str()), (control.port, control.token()));
        let ui = std::thread::spawn(move || {
            let mut answered = 0;
            while answered < 3 {
                for p in control.take() {
                    let outcome = match p.call.method.as_str() {
                        "app.screenshot" => Ok(Answer::Png(vec![137, 80, 78, 71])),
                        "model.geometry" => Err(Error::BadParams("model.geometry: unknown field `x`".into())),
                        _ => Ok(Answer::Json(json!({ "params": p.call.params, "client": p.call.client }))),
                    };
                    p.answer(outcome);
                    answered += 1;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            control
        });
        assert_eq!(call(port, &token, &Call::new("app.ping", Value::Null)), Ok(Answer::Json(json!({}))));
        assert_eq!(call(port, &token, &Call::new("app.screenshot", Value::Null)), Ok(Answer::Png(vec![137, 80, 78, 71])));
        assert!(
            matches!(call(port, &token, &Call::new("model.geometry", json!({ "x": 1 }))), Err(Error::BadParams(m)) if m.contains("`x`"))
        );
        // The client named, its name on one line.
        let named = Call::new("app.state", json!({ "a": 1 })).from_client(Some("mcp\r\nX-Evil: 1".into()));
        assert_eq!(call(port, &token, &named), Ok(Answer::Json(json!({ "params": { "a": 1 }, "client": "mcpX-Evil: 1" }))));
        assert!(matches!(call(port, "00", &Call::new("app.state", Value::Null)), Err(Error::Failed(m)) if m.contains("token")));
        let control = ui.join().unwrap();
        drop(control);
        assert!(matches!(call(port, &token, &Call::new("app.state", Value::Null)), Err(Error::Unavailable(_))));
        assert!(discover(&dir, None).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

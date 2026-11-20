//! Where the MCP server's tool calls go (docs/control-protocol.md): the running app,
//! over its control channel ([`Remote`]), or an editor of the server's own, without a window
//! ([`Headless`]).

use std::path::PathBuf;

use serde_json::{Value, json};
use skraft_control::params::NoParams;
use skraft_control::{Answer, Call, Error, Method, Outcome};
use skraft_model::Document;
use skraft_tools::Editor;

pub trait Backend {
    fn call(&mut self, call: &Call) -> Outcome;
    /// Whether it serves `m` (offered as a tool).
    fn serves(&self, m: &Method) -> bool;
    /// What it is, for the server's instructions.
    fn describe(&self) -> String;
}

/// The app running on this computer, its channel found anew at every call: an app started,
/// or started again, after the server is. Several may run (one per worktree): the one this
/// server's Claude Code session launched, else one built from the worktree it runs in
/// (`skraft_control::instances::choose`), else the last started (the discovery file).
pub struct Remote {
    dir: PathBuf,
    port: Option<u16>,
    /// Its name for the app's journal.
    client: String,
}

impl Remote {
    /// The app this server means (see [`Remote`]), or the one on `port`.
    pub fn new(port: Option<u16>) -> Self {
        Remote {
            dir: skraft_control::server::default_dir().unwrap_or_default(),
            port,
            client: skraft_control::instances::client_name("mcp"),
        }
    }

    /// Does an app answer?
    pub fn reachable(&self) -> bool {
        self.ping().is_ok()
    }

    fn ping(&self) -> Outcome {
        let (port, token) = self.find().map_err(Error::Unavailable)?;
        skraft_control::server::call(port, &token, &Call::new("app.ping", Value::Null))
    }

    /// The port and token of the app meant.
    fn find(&self) -> Result<(u16, String), String> {
        let port = self.port.or_else(|| {
            let all = skraft_control::instances::list(&self.dir);
            let session = std::env::var("CLAUDE_CODE_SESSION_ID").ok();
            let cwd = std::env::current_dir().ok();
            skraft_control::instances::choose(&all, session.as_deref(), cwd.as_deref()).and_then(|i| i.port)
        });
        skraft_control::server::discover(&self.dir, port)
    }
}

impl Backend for Remote {
    fn call(&mut self, call: &Call) -> Outcome {
        if call.method == "app.instances" {
            call.params::<NoParams>()?;
            return Ok(Answer::Json(skraft_control::instances::list_json(&self.dir)));
        }
        let (port, token) = self
            .find()
            .map_err(|e| Error::Unavailable(format!("{e}: start Skraft with its control channel on (title bar, or SKRAFT_CONTROL=1)")))?;
        skraft_control::server::call(port, &token, &call.clone().from_client(Some(self.client.clone())))
    }

    fn serves(&self, _: &Method) -> bool {
        true
    }

    fn describe(&self) -> String {
        let at = match self.port {
            Some(p) => format!("port {p}"),
            None => "the one this Claude Code session launched, else one built from this worktree, else the last started — `app_state` says which (`instance`), `app_instances` lists them all".into(),
        };
        format!(
            "connected to the running Skraft app ({at}): what it does shows in its window, every tool is there. Its journal marks what this server does as the pilot's ({}).",
            self.client
        )
    }
}

/// An editor of the server's own: a document (empty, or read from a file), the kernel run
/// inline, a virtual viewport of 1280 × 800 — every method but the window's (`ui.*`, commands,
/// screenshots).
pub struct Headless {
    editor: Editor,
    path: Option<PathBuf>,
}

impl Headless {
    pub fn new(path: Option<PathBuf>) -> Result<Self, String> {
        let doc = match &path {
            Some(p) => {
                let bytes = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
                Document::from_file(&bytes).map_err(|e| format!("{}: {e}", p.display()))?
            }
            None => Document::new(),
        };
        let mut editor = Editor::new(doc, Box::new(skraft_kernel::Inline::default()));
        // Whatever it goes through, the client asked for.
        editor.set_origin(skraft_tools::journal::Origin::control(Some("mcp · headless")));
        editor.set_viewport_size(glam::DVec2::new(1280.0, 800.0));
        editor.zoom_extents();
        Ok(Headless { editor, path })
    }

    fn stamp(&self) -> Value {
        let e = &self.editor;
        json!({ "document": 1, "revision": e.history().revision(), "busy": e.busy(), "idle": skraft_control::editor::idle(e) })
    }
}

impl Backend for Headless {
    fn call(&mut self, call: &Call) -> Outcome {
        if let Some(outcome) = skraft_control::editor::call(&mut self.editor, call) {
            return outcome;
        }
        let e = &mut self.editor;
        match call.method.as_str() {
            "app.ping" => Ok(Answer::Json(json!({}))),
            "app.methods" => Ok(Answer::Json(skraft_control::methods_json())),
            "app.instances" => {
                call.params::<NoParams>()?;
                let dir = skraft_control::server::default_dir().unwrap_or_default();
                Ok(Answer::Json(skraft_control::instances::list_json(&dir)))
            }
            "app.state" => {
                call.params::<NoParams>()?;
                let mut state = skraft_control::editor::state(e);
                state["modified"] = json!(e.history().revision() > 0);
                state["path"] = json!(self.path);
                state["document"] = json!(1);
                state["headless"] = json!(true);
                Ok(Answer::Json(state))
            }
            "app.wait" => {
                call.params::<skraft_control::params::WaitParams>()?;
                // The kernel runs inline: what was asked is done.
                e.poll();
                let mut v = self.stamp();
                v["timed_out"] = json!(false);
                Ok(Answer::Json(v))
            }
            m if skraft_control::is_method(m) => Err(Error::Unavailable(format!(
                "{m} needs the app's window: start Skraft with its control channel on, then this server without --headless"
            ))),
            m => Err(Error::UnknownMethod(m.into())),
        }
    }

    fn serves(&self, m: &Method) -> bool {
        m.headless
    }

    fn describe(&self) -> String {
        let doc = match &self.path {
            Some(p) => format!("the document {}", p.display()),
            None => "an empty document".into(),
        };
        format!(
            "headless — an editor of its own on {doc}, no window (no Skraft app was running, or --headless): the model, the tools and the camera, not the interface (ui_*), commands nor screenshots."
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json_of(b: &mut Headless, method: &str, params: Value) -> Value {
        match b.call(&Call::new(method, params)) {
            Ok(Answer::Json(v)) => v,
            other => panic!("{method}: {other:?}"),
        }
    }

    #[test]
    fn headless_models_with_the_tools() {
        let mut b = Headless::new(None).unwrap();
        assert_eq!(json_of(&mut b, "app.state", Value::Null)["tool"], "select");
        json_of(
            &mut b,
            "tool.input",
            json!({ "inputs": [{ "tool": "box" }, { "click": { "at": [0, 0, 0] } }, { "click": { "at": [600, 400, 0] } }, { "vcb": "18" }, { "tool": "select" }] }),
        );
        assert_eq!(json_of(&mut b, "app.wait", json!({ "idle": true }))["revision"], 1);
        let g = json_of(&mut b, "model.geometry", Value::Null);
        let dims: Vec<f64> = serde_json::from_value(g["parts"][0]["dims"].clone()).unwrap();
        assert!((dims[0] - 600.0).abs() < 1e-9 && (dims[1] - 400.0).abs() < 1e-9 && (dims[2] - 18.0).abs() < 1e-9, "{dims:?}");
        assert!((g["parts"][0]["volume"].as_f64().unwrap() - 600.0 * 400.0 * 18.0).abs() < 1.0);
        // The window's methods: refused, said why.
        assert!(matches!(b.call(&Call::new("ui.click", json!({}))), Err(Error::Unavailable(m)) if m.contains("window")));
        assert!(matches!(b.call(&Call::new("app.screenshot", Value::Null)), Err(Error::Unavailable(_))));
    }

    #[test]
    fn headless_opens_a_file() {
        let mut b = Headless::new(None).unwrap();
        json_of(
            &mut b,
            "tool.input",
            json!({ "inputs": [{ "tool": "box" }, { "click": { "at": [0, 0, 0] } }, { "click": { "at": [100, 100, 0] } }, { "vcb": "10" }] }),
        );
        let file = b.editor.document().to_file().unwrap();
        let path = std::env::temp_dir().join(format!("skraft-mcp-{}.skraft", std::process::id()));
        std::fs::write(&path, file).unwrap();
        let mut opened = Headless::new(Some(path.clone())).unwrap();
        assert_eq!(json_of(&mut opened, "model.geometry", json!({ "volume": false }))["total"], 1);
        assert!(opened.describe().contains(&path.display().to_string()));
        let _ = std::fs::remove_file(&path);
        assert!(Headless::new(Some(path)).is_err());
    }
}
